use crate::bank_forks::BankForks;
use crate::blocktree::Blocktree;
use crate::entry::{Entry, EntrySlice};
use crate::leader_schedule_utils;
use rayon::prelude::*;
use solana_metrics::counter::Counter;
use solana_runtime::bank::{Bank, BankError, Result};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::timing::duration_as_ms;
use solana_sdk::timing::MAX_RECENT_TICK_HASHES;
use std::sync::Arc;
use std::time::Instant;

pub fn process_entry(bank: &Bank, entry: &Entry) -> Result<()> {
    if !entry.is_tick() {
        first_err(&bank.process_transactions(&entry.transactions))?;
    } else {
        bank.register_tick(&entry.hash);
    }
    Ok(())
}

fn first_err(results: &[Result<()>]) -> Result<()> {
    for r in results {
        r.clone()?;
    }
    Ok(())
}

fn par_execute_entries(bank: &Bank, entries: &[(&Entry, Vec<Result<()>>)]) -> Result<()> {
    inc_new_counter_info!("bank-par_execute_entries-count", entries.len());
    let results: Vec<Result<()>> = entries
        .into_par_iter()
        .map(|(e, lock_results)| {
            let results = bank.load_execute_and_commit_transactions(
                &e.transactions,
                lock_results.to_vec(),
                MAX_RECENT_TICK_HASHES,
            );
            bank.unlock_accounts(&e.transactions, &results);
            first_err(&results)
        })
        .collect();

    first_err(&results)
}

/// process entries in parallel
/// 1. In order lock accounts for each entry while the lock succeeds, up to a Tick entry
/// 2. Process the locked group in parallel
/// 3. Register the `Tick` if it's available
/// 4. Update the leader scheduler, goto 1
fn par_process_entries(bank: &Bank, entries: &[Entry]) -> Result<()> {
    // accumulator for entries that can be processed in parallel
    let mut mt_group = vec![];
    for entry in entries {
        if entry.is_tick() {
            // if its a tick, execute the group and register the tick
            par_execute_entries(bank, &mt_group)?;
            bank.register_tick(&entry.hash);
            mt_group = vec![];
            continue;
        }
        // try to lock the accounts
        let lock_results = bank.lock_accounts(&entry.transactions);
        // if any of the locks error out
        // execute the current group
        if first_err(&lock_results).is_err() {
            par_execute_entries(bank, &mt_group)?;
            mt_group = vec![];
            //reset the lock and push the entry
            bank.unlock_accounts(&entry.transactions, &lock_results);
            let lock_results = bank.lock_accounts(&entry.transactions);
            mt_group.push((entry, lock_results));
        } else {
            // push the entry to the mt_group
            mt_group.push((entry, lock_results));
        }
    }
    par_execute_entries(bank, &mt_group)?;
    Ok(())
}

/// Process an ordered list of entries.
pub fn process_entries(bank: &Bank, entries: &[Entry]) -> Result<()> {
    par_process_entries(bank, entries)
}

/// Process an ordered list of entries, populating a circular buffer "tail"
/// as we go.
fn process_block(bank: &Bank, entries: &[Entry]) -> Result<()> {
    for entry in entries {
        process_entry(bank, entry)?;
    }

    Ok(())
}

#[derive(Debug, PartialEq)]
pub struct BankForksInfo {
    pub bank_id: u64,
    pub entry_height: u64,
}

pub fn process_blocktree(
    genesis_block: &GenesisBlock,
    blocktree: &Blocktree,
    account_paths: Option<String>,
) -> Result<(BankForks, Vec<BankForksInfo>)> {
    let now = Instant::now();
    info!("processing ledger...");

    // Setup bank for slot 0
    let mut pending_slots = {
        let slot = 0;
        let bank = Arc::new(Bank::new_with_paths(&genesis_block, account_paths));
        let entry_height = 0;
        let last_entry_hash = bank.last_block_hash();
        vec![(slot, bank, entry_height, last_entry_hash)]
    };

    let mut fork_info = vec![];
    while !pending_slots.is_empty() {
        let (slot, starting_bank, starting_entry_height, mut last_entry_hash) =
            pending_slots.pop().unwrap();

        let bank = Arc::new(Bank::new_from_parent(
            &starting_bank,
            leader_schedule_utils::slot_leader_at(slot, &starting_bank),
            starting_bank.slot(),
        ));
        let mut entry_height = starting_entry_height;

        // Load the metadata for this slot
        let meta = blocktree
            .meta(slot)
            .map_err(|err| {
                warn!("Failed to load meta for slot {}: {:?}", slot, err);
                BankError::LedgerVerificationFailed
            })?
            .unwrap();

        // Fetch all entries for this slot
        let mut entries = blocktree.get_slot_entries(slot, 0, None).map_err(|err| {
            warn!("Failed to load entries for slot {}: {:?}", slot, err);
            BankError::LedgerVerificationFailed
        })?;

        if slot == 0 {
            // The first entry in the ledger is a pseudo-tick used only to ensure the number of ticks
            // in slot 0 is the same as the number of ticks in all subsequent slots.  It is not
            // processed by the bank, skip over it.
            if entries.is_empty() {
                warn!("entry0 not present");
                return Err(BankError::LedgerVerificationFailed);
            }
            let entry0 = &entries[0];
            if !(entry0.is_tick() && entry0.verify(&last_entry_hash)) {
                warn!("Ledger proof of history failed at entry0");
                return Err(BankError::LedgerVerificationFailed);
            }
            last_entry_hash = entry0.hash;
            entry_height += 1;
            entries = entries.drain(1..).collect();
        }

        if !entries.is_empty() {
            if !entries.verify(&last_entry_hash) {
                warn!("Ledger proof of history failed at entry: {}", entry_height);
                return Err(BankError::LedgerVerificationFailed);
            }

            process_block(&bank, &entries).map_err(|err| {
                warn!("Failed to process entries for slot {}: {:?}", slot, err);
                BankError::LedgerVerificationFailed
            })?;

            last_entry_hash = entries.last().unwrap().hash;
            entry_height += entries.len() as u64;
        }

        let slot_complete =
            leader_schedule_utils::num_ticks_left_in_slot(&bank, bank.tick_height()) == 0;
        if !slot_complete {
            // Slot was not complete, clear out any partial entries
            // TODO: Walk |meta.next_slots| and clear all child slots too?
            blocktree.reset_slot_consumed(slot).map_err(|err| {
                warn!("Failed to reset partial slot {}: {:?}", slot, err);
                BankError::LedgerVerificationFailed
            })?;

            let bfi = BankForksInfo {
                bank_id: slot,
                entry_height: starting_entry_height,
            };
            fork_info.push((starting_bank, bfi));
            continue;
        }

        // TODO merge with locktower, voting
        bank.squash();

        if meta.next_slots.is_empty() {
            // Reached the end of this fork.  Record the final entry height and last entry.hash

            let bfi = BankForksInfo {
                bank_id: slot,
                entry_height,
            };
            fork_info.push((bank, bfi));
            continue;
        }

        // This is a fork point, create a new child bank for each fork
        pending_slots.extend(meta.next_slots.iter().map(|next_slot| {
            let child_bank = Arc::new(Bank::new_from_parent(
                &bank,
                leader_schedule_utils::slot_leader_at(*next_slot, &bank),
                *next_slot,
            ));
            trace!("Add child bank for slot={}", next_slot);
            // bank_forks.insert(*next_slot, child_bank);
            (*next_slot, child_bank, entry_height, last_entry_hash)
        }));

        // reverse sort by slot, so the next slot to be processed can be pop()ed
        // TODO: remove me once leader_scheduler can hang with out-of-order slots?
        pending_slots.sort_by(|a, b| b.0.cmp(&a.0));
    }

    let (banks, bank_forks_info): (Vec<_>, Vec<_>) = fork_info.into_iter().unzip();
    let bank_forks = BankForks::new_from_banks(&banks);
    info!(
        "processed ledger in {}ms, forks={}...",
        duration_as_ms(&now.elapsed()),
        bank_forks_info.len(),
    );

    Ok((bank_forks, bank_forks_info))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree::create_new_tmp_ledger;
    use crate::blocktree::tests::entries_to_blobs;
    use crate::entry::{create_ticks, next_entry, Entry};
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction::SystemTransaction;

    fn fill_blocktree_slot_with_ticks(
        blocktree: &Blocktree,
        ticks_per_slot: u64,
        slot: u64,
        parent_slot: u64,
        last_entry_hash: Hash,
    ) -> Hash {
        let entries = create_ticks(ticks_per_slot, last_entry_hash);
        let last_entry_hash = entries.last().unwrap().hash;

        let blobs = entries_to_blobs(&entries, slot, parent_slot);
        blocktree.insert_data_blobs(blobs.iter()).unwrap();

        last_entry_hash
    }

    #[test]
    fn test_process_blocktree_with_incomplete_slot() {
        solana_logger::setup();

        let (genesis_block, _mint_keypair) = GenesisBlock::new(10_000);
        let ticks_per_slot = genesis_block.ticks_per_slot;

        /*
          Build a blocktree in the ledger with the following fork structure:

               slot 0
                 |
               slot 1
                 |
               slot 2

           where slot 1 is incomplete (missing 1 tick at the end)
        */

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, mut block_hash) = create_new_tmp_ledger!(&genesis_block);
        debug!("ledger_path: {:?}", ledger_path);

        let blocktree = Blocktree::open_config(&ledger_path, ticks_per_slot)
            .expect("Expected to successfully open database ledger");

        // Write slot 1
        // slot 1, points at slot 0.  Missing one tick
        {
            let parent_slot = 0;
            let slot = 1;
            let mut entries = create_ticks(ticks_per_slot, block_hash);
            block_hash = entries.last().unwrap().hash;

            entries.pop();

            let blobs = entries_to_blobs(&entries, slot, parent_slot);
            blocktree.insert_data_blobs(blobs.iter()).unwrap();
        }

        // slot 2, points at slot 1
        fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 2, 1, block_hash);

        let (mut _bank_forks, bank_forks_info) =
            process_blocktree(&genesis_block, &blocktree, None).unwrap();

        assert_eq!(bank_forks_info.len(), 1);
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_id: 1, // never finished first slot
                entry_height: ticks_per_slot,
            }
        );
    }

    #[test]
    fn test_process_blocktree_with_two_forks() {
        solana_logger::setup();

        let (genesis_block, _mint_keypair) = GenesisBlock::new(10_000);
        let ticks_per_slot = genesis_block.ticks_per_slot;

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, block_hash) = create_new_tmp_ledger!(&genesis_block);
        debug!("ledger_path: {:?}", ledger_path);
        let mut last_entry_hash = block_hash;

        /*
            Build a blocktree in the ledger with the following fork structure:

                 slot 0
                   |
                 slot 1
                 /   \
            slot 2   |
               /     |
            slot 3   |
                     |
                   slot 4

        */
        let blocktree = Blocktree::open_config(&ledger_path, ticks_per_slot)
            .expect("Expected to successfully open database ledger");

        // Fork 1, ending at slot 3
        let last_slot1_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 1, 0, last_entry_hash);
        last_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 2, 1, last_slot1_entry_hash);
        let last_fork1_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 3, 2, last_entry_hash);

        // Fork 2, ending at slot 4
        let last_fork2_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 4, 1, last_slot1_entry_hash);

        info!("last_fork1_entry.hash: {:?}", last_fork1_entry_hash);
        info!("last_fork2_entry.hash: {:?}", last_fork2_entry_hash);

        let (bank_forks, bank_forks_info) =
            process_blocktree(&genesis_block, &blocktree, None).unwrap();

        assert_eq!(bank_forks_info.len(), 2); // There are two forks
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_id: 3, // Fork 1's head is slot 3
                entry_height: ticks_per_slot * 4,
            }
        );
        assert_eq!(
            bank_forks_info[1],
            BankForksInfo {
                bank_id: 4, // Fork 2's head is slot 4
                entry_height: ticks_per_slot * 3,
            }
        );

        // Ensure bank_forks holds the right banks
        for info in bank_forks_info {
            assert_eq!(bank_forks[info.bank_id].slot(), info.bank_id);
        }
    }

    #[test]
    fn test_first_err() {
        assert_eq!(first_err(&[Ok(())]), Ok(()));
        assert_eq!(
            first_err(&[Ok(()), Err(BankError::DuplicateSignature)]),
            Err(BankError::DuplicateSignature)
        );
        assert_eq!(
            first_err(&[
                Ok(()),
                Err(BankError::DuplicateSignature),
                Err(BankError::AccountInUse)
            ]),
            Err(BankError::DuplicateSignature)
        );
        assert_eq!(
            first_err(&[
                Ok(()),
                Err(BankError::AccountInUse),
                Err(BankError::DuplicateSignature)
            ]),
            Err(BankError::AccountInUse)
        );
        assert_eq!(
            first_err(&[
                Err(BankError::AccountInUse),
                Ok(()),
                Err(BankError::DuplicateSignature)
            ]),
            Err(BankError::AccountInUse)
        );
    }

    #[test]
    fn test_process_empty_entry_is_registered() {
        solana_logger::setup();

        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let bank = Bank::new(&genesis_block);
        let keypair = Keypair::new();
        let slot_entries = create_ticks(genesis_block.ticks_per_slot - 1, genesis_block.hash());
        let tx = SystemTransaction::new_account(
            &mint_keypair,
            keypair.pubkey(),
            1,
            slot_entries.last().unwrap().hash,
            0,
        );

        // First, ensure the TX is rejected because of the unregistered last ID
        assert_eq!(
            bank.process_transaction(&tx),
            Err(BankError::BlockHashNotFound)
        );

        // Now ensure the TX is accepted despite pointing to the ID of an empty entry.
        par_process_entries(&bank, &slot_entries).unwrap();
        assert_eq!(bank.process_transaction(&tx), Ok(()));
    }

    #[test]
    fn test_process_ledger_simple() {
        solana_logger::setup();
        let leader_pubkey = Keypair::new().pubkey();
        let (genesis_block, mint_keypair) = GenesisBlock::new_with_leader(100, leader_pubkey, 50);
        let (ledger_path, mut last_entry_hash) = create_new_tmp_ledger!(&genesis_block);
        debug!("ledger_path: {:?}", ledger_path);

        let mut entries = vec![];
        let block_hash = genesis_block.hash();
        for _ in 0..3 {
            // Transfer one token from the mint to a random account
            let keypair = Keypair::new();
            let tx =
                SystemTransaction::new_account(&mint_keypair, keypair.pubkey(), 1, block_hash, 0);
            let entry = Entry::new(&last_entry_hash, 1, vec![tx]);
            last_entry_hash = entry.hash;
            entries.push(entry);

            // Add a second Transaction that will produce a
            // ProgramError<0, ResultWithNegativeTokens> error when processed
            let keypair2 = Keypair::new();
            let tx = SystemTransaction::new_account(&keypair, keypair2.pubkey(), 42, block_hash, 0);
            let entry = Entry::new(&last_entry_hash, 1, vec![tx]);
            last_entry_hash = entry.hash;
            entries.push(entry);
        }

        // Fill up the rest of slot 1 with ticks
        entries.extend(create_ticks(genesis_block.ticks_per_slot, last_entry_hash));

        let blocktree =
            Blocktree::open(&ledger_path).expect("Expected to successfully open database ledger");
        blocktree.write_entries(1, 0, 0, &entries).unwrap();
        let entry_height = genesis_block.ticks_per_slot + entries.len() as u64;
        let (bank_forks, bank_forks_info) =
            process_blocktree(&genesis_block, &blocktree, None).unwrap();

        assert_eq!(bank_forks_info.len(), 1);
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_id: 1,
                entry_height,
            }
        );

        let bank = bank_forks[1].clone();
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 50 - 3);
        assert_eq!(bank.tick_height(), 2 * genesis_block.ticks_per_slot - 1);
        assert_eq!(bank.last_block_hash(), entries.last().unwrap().hash);
    }

    #[test]
    fn test_process_ledger_with_one_tick_per_slot() {
        let (mut genesis_block, _mint_keypair) = GenesisBlock::new(123);
        genesis_block.ticks_per_slot = 1;
        let (ledger_path, _block_hash) = create_new_tmp_ledger!(&genesis_block);

        let blocktree = Blocktree::open(&ledger_path).unwrap();
        let (bank_forks, bank_forks_info) =
            process_blocktree(&genesis_block, &blocktree, None).unwrap();

        assert_eq!(bank_forks_info.len(), 1);
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_id: 0,
                entry_height: 1,
            }
        );
        let bank = bank_forks[0].clone();
        assert_eq!(bank.tick_height(), 0);
    }

    #[test]
    fn test_par_process_entries_tick() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);

        // ensure bank can process a tick
        assert_eq!(bank.tick_height(), 0);
        let tick = next_entry(&genesis_block.hash(), 1, vec![]);
        assert_eq!(par_process_entries(&bank, &[tick.clone()]), Ok(()));
        assert_eq!(bank.tick_height(), 1);
    }

    #[test]
    fn test_par_process_entries_2_entries_collision() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        let block_hash = bank.last_block_hash();

        // ensure bank can process 2 entries that have a common account and no tick is registered
        let tx = SystemTransaction::new_account(
            &mint_keypair,
            keypair1.pubkey(),
            2,
            bank.last_block_hash(),
            0,
        );
        let entry_1 = next_entry(&block_hash, 1, vec![tx]);
        let tx = SystemTransaction::new_account(
            &mint_keypair,
            keypair2.pubkey(),
            2,
            bank.last_block_hash(),
            0,
        );
        let entry_2 = next_entry(&entry_1.hash, 1, vec![tx]);
        assert_eq!(par_process_entries(&bank, &[entry_1, entry_2]), Ok(()));
        assert_eq!(bank.get_balance(&keypair1.pubkey()), 2);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 2);
        assert_eq!(bank.last_block_hash(), block_hash);
    }

    #[test]
    fn test_par_process_entries_2_txes_collision() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();

        // fund: put 4 in each of 1 and 2
        assert_matches!(
            bank.transfer(4, &mint_keypair, keypair1.pubkey(), bank.last_block_hash()),
            Ok(_)
        );
        assert_matches!(
            bank.transfer(4, &mint_keypair, keypair2.pubkey(), bank.last_block_hash()),
            Ok(_)
        );

        // construct an Entry whose 2nd transaction would cause a lock conflict with previous entry
        let entry_1_to_mint = next_entry(
            &bank.last_block_hash(),
            1,
            vec![SystemTransaction::new_account(
                &keypair1,
                mint_keypair.pubkey(),
                1,
                bank.last_block_hash(),
                0,
            )],
        );

        let entry_2_to_3_mint_to_1 = next_entry(
            &entry_1_to_mint.hash,
            1,
            vec![
                SystemTransaction::new_account(
                    &keypair2,
                    keypair3.pubkey(),
                    2,
                    bank.last_block_hash(),
                    0,
                ), // should be fine
                SystemTransaction::new_account(
                    &keypair1,
                    mint_keypair.pubkey(),
                    2,
                    bank.last_block_hash(),
                    0,
                ), // will collide
            ],
        );

        assert_eq!(
            par_process_entries(&bank, &[entry_1_to_mint, entry_2_to_3_mint_to_1]),
            Ok(())
        );

        assert_eq!(bank.get_balance(&keypair1.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 2);
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 2);
    }

    #[test]
    fn test_par_process_entries_2_entries_par() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        //load accounts
        let tx = SystemTransaction::new_account(
            &mint_keypair,
            keypair1.pubkey(),
            1,
            bank.last_block_hash(),
            0,
        );
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        let tx = SystemTransaction::new_account(
            &mint_keypair,
            keypair2.pubkey(),
            1,
            bank.last_block_hash(),
            0,
        );
        assert_eq!(bank.process_transaction(&tx), Ok(()));

        // ensure bank can process 2 entries that do not have a common account and no tick is registered
        let block_hash = bank.last_block_hash();
        let tx = SystemTransaction::new_account(
            &keypair1,
            keypair3.pubkey(),
            1,
            bank.last_block_hash(),
            0,
        );
        let entry_1 = next_entry(&block_hash, 1, vec![tx]);
        let tx = SystemTransaction::new_account(
            &keypair2,
            keypair4.pubkey(),
            1,
            bank.last_block_hash(),
            0,
        );
        let entry_2 = next_entry(&entry_1.hash, 1, vec![tx]);
        assert_eq!(par_process_entries(&bank, &[entry_1, entry_2]), Ok(()));
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair4.pubkey()), 1);
        assert_eq!(bank.last_block_hash(), block_hash);
    }

    #[test]
    fn test_par_process_entries_2_entries_tick() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        //load accounts
        let tx = SystemTransaction::new_account(
            &mint_keypair,
            keypair1.pubkey(),
            1,
            bank.last_block_hash(),
            0,
        );
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        let tx = SystemTransaction::new_account(
            &mint_keypair,
            keypair2.pubkey(),
            1,
            bank.last_block_hash(),
            0,
        );
        assert_eq!(bank.process_transaction(&tx), Ok(()));

        let block_hash = bank.last_block_hash();
        while block_hash == bank.last_block_hash() {
            bank.register_tick(&Hash::default());
        }

        // ensure bank can process 2 entries that do not have a common account and tick is registered
        let tx = SystemTransaction::new_account(&keypair2, keypair3.pubkey(), 1, block_hash, 0);
        let entry_1 = next_entry(&block_hash, 1, vec![tx]);
        let tick = next_entry(&entry_1.hash, 1, vec![]);
        let tx = SystemTransaction::new_account(
            &keypair1,
            keypair4.pubkey(),
            1,
            bank.last_block_hash(),
            0,
        );
        let entry_2 = next_entry(&tick.hash, 1, vec![tx]);
        assert_eq!(
            par_process_entries(&bank, &[entry_1.clone(), tick.clone(), entry_2.clone()]),
            Ok(())
        );
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair4.pubkey()), 1);

        // ensure that an error is returned for an empty account (keypair2)
        let tx = SystemTransaction::new_account(
            &keypair2,
            keypair3.pubkey(),
            1,
            bank.last_block_hash(),
            0,
        );
        let entry_3 = next_entry(&entry_2.hash, 1, vec![tx]);
        assert_eq!(
            par_process_entries(&bank, &[entry_3]),
            Err(BankError::AccountNotFound)
        );
    }
}