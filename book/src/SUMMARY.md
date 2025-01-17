# Solana Architecture

- [Introduction](introduction.md)

- [Terminology](terminology.md)

- [Getting Started](getting-started.md)
  - [Testnet Participation](testnet-participation.md)
  - [Example Client: Web Wallet](webwallet.md)

- [Programming Model](programs.md)
  - [Example: Tic-Tac-Toe](tictactoe.md)
  - [Drones](drones.md)

- [A Solana Cluster](cluster.md)
  - [Synchronization](synchronization.md)
  - [Leader Rotation](leader-rotation.md)
  - [Fork Generation](fork-generation.md)
  - [Managing Forks](managing-forks.md)
  - [Turbine Block Propagation](turbine-block-propagation.md)
  - [Ledger Replication](ledger-replication.md)
  - [Secure Vote Signing](vote-signing.md)
  - [Stake Delegation and Rewards](stake-delegation-and-rewards.md)
  - [Performance Metrics](performance-metrics.md)

- [Anatomy of a Validator](validator.md)
  - [TPU](tpu.md)
  - [TVU](tvu.md)
    - [Blocktree](blocktree.md)
  - [Gossip Service](gossip.md)
  - [The Runtime](runtime.md)

- [Anatomy of a Transaction](transaction.md)

- [Running a Validator](running-validator.md)
  - [Hardware Requirements](validator-hardware.md)
  - [Choosing a Testnet](validator-testnet.md)
  - [Installing the Validator Software](validator-software.md)
  - [Starting a Validator](validator-start.md)
  - [Staking](validator-stake.md)
  - [Monitoring a Validator](validator-monitor.md)
  - [Publishing Validator Info](validator-info.md)
  - [Troubleshooting](validator-troubleshoot.md)
  - [FAQ](validator-faq.md)

- [Running a Replicator](running-replicator.md)

- [API Reference](api-reference.md)
  - [Transaction](transaction-api.md)
  - [Instruction](instruction-api.md)
  - [Blockstreamer](blockstreamer.md)
  - [JSON RPC API](jsonrpc-api.md)
  - [JavaScript API](javascript-api.md)
  - [solana-wallet CLI](wallet.md)

- [Accepted Design Proposals](proposals.md)
  - [Ledger Replication](ledger-replication-to-implement.md)
  - [Secure Vote Signing](vote-signing-to-implement.md)
  - [Staking Rewards](staking-rewards.md)
  - [Cluster Economics](ed_overview.md)
    - [Validation-client Economics](ed_validation_client_economics.md)
      - [State-validation Protocol-based Rewards](ed_vce_state_validation_protocol_based_rewards.md)
      - [State-validation Transaction Fees](ed_vce_state_validation_transaction_fees.md)
      - [Replication-validation Transaction Fees](ed_vce_replication_validation_transaction_fees.md)
      - [Validation Stake Delegation](ed_vce_validation_stake_delegation.md)
    - [Replication-client Economics](ed_replication_client_economics.md)
      - [Storage-replication Rewards](ed_rce_storage_replication_rewards.md)
      - [Replication-client Reward Auto-delegation](ed_rce_replication_client_reward_auto_delegation.md)
    - [Economic Sustainability](ed_economic_sustainability.md)
    - [Attack Vectors](ed_attack_vectors.md)
	- [Economic Design MVP](ed_mvp.md)
    - [References](ed_references.md)
  - [Cluster Test Framework](cluster-test-framework.md)
  - [Validator](validator-proposal.md)
  - [Simple Payment and State Verification](simple-payment-and-state-verification.md)
  - [Cross-Program Invocation](cross-program-invocation.md)

- [Implemented Design Proposals](implemented-proposals.md)
  - [Blocktree](blocktree.md)
  - [Cluster Software Installation and Updates](installer.md)
  - [Deterministic Transaction Fees](transaction-fees.md)
  - [Tower BFT](tower-bft.md)
  - [Leader-to-Leader Transition](leader-leader-transition.md)
  - [Leader-to-Validator Transition](leader-validator-transition.md)
  - [Passive Stake Delegation and Rewards](passive-stake-delegation-and-rewards.md)
  - [Persistent Account Storage](persistent-account-storage.md)
  - [Reliable Vote Transmission](reliable-vote-transmission.md)
  - [Repair Service](repair-service.md)
  - [Testing Programs](testing-programs.md)
  - [Credit-only Accounts](credit-only-credit-debit-accounts.md)
  - [Embedding the Move Langauge](embedding-move.md)
