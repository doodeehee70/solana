# Starting a Validator

## Confirm The Testnet Is Reachable
Before attaching a validator node, sanity check that the cluster is accessible
to your machine by running some simple commands.  If any of the commands fail,
please retry 5-10 minutes later to confirm the testnet is not just restarting
itself before debugging further.

Fetch the current transaction count over JSON RPC:
```bash
$ curl -X POST -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' http://testnet.solana.com:8899
```

Inspect the network explorer at
[https://explorer.solana.com/](https://explorer.solana.com/) for activity.

View the [metrics dashboard](
https://metrics.solana.com:3000/d/testnet-beta/testnet-monitor-beta?var-testnet=testnet)
for more detail on cluster activity.

## Confirm your Installation
Sanity check that you are able to interact with the cluster by receiving a small
airdrop of lamports from the testnet drone:
```bash
$ solana-wallet airdrop 123
$ solana-wallet balance
```

Also try running following command to join the gossip network and view all the
other nodes in the cluster:
```bash
$ solana-gossip --entrypoint testnet.solana.com:8001 spy
# Press ^C to exit
```

## Start your Validator
Now create an identity keypair for your validator by running:
```bash
$ solana-keygen new -o ~/validator-keypair.json
```
and airdrop yourself some lamports to get started:
```bash
$ solana-wallet --keypair ~/validator-keypair.json airdrop 1000
```

Your validator will need a vote account.  Create it now with the following
commands:
```bash
$ solana-keygen new -o ~/validator-vote-keypair.json
$ solana-wallet --keypair ~/validator-keypair.json create-vote-account ~/validator-vote-keypair.json ~/validator-keypair.json 1
```

Then use one of the following commands, depending on your installation
choice, to start the node:

If this is a `solana-install`-installation:
```bash
$ validator.sh --identity ~/validator-keypair.json --voting-keypair ~/validator-vote-keypair.json --ledger ~/validator-config --rpc-port 8899 --poll-for-new-genesis-block testnet.solana.com
```

Alternatively, the `solana-install run` command can be used to run the validator
node while periodically checking for and applying software updates:
```bash
$ solana-install run validator.sh -- --identity ~/validator-keypair.json --voting-keypair ~/validator-vote-keypair.json --ledger ~/validator-config --rpc-port 8899 --poll-for-new-genesis-block testnet.solana.com
```

If you built from source:
```bash
$ NDEBUG=1 USE_INSTALL=1 ./multinode-demo/validator.sh --identity ~/validator-keypair.json --voting-keypair ~/validator-vote-keypair.json --rpc-port 8899 --poll-for-new-genesis-block testnet.solana.com
```

### Enabling CUDA
By default CUDA is disabled.  If your machine has a GPU with CUDA installed,
define the SOLANA_CUDA flag in your environment *before* running any of the
previusly mentioned commands
```bash
$ export SOLANA_CUDA=1
```

When your validator is started look for the following log message to indicate that CUDA is enabled:
`"[<timestamp> solana::validator] CUDA is enabled"`

### Controlling local network port allocation
By default the validator will dynamically select available network ports in the
8000-10000 range, and may be overridden with `--dynamic-port-range`.  For
example, `validator.sh --dynamic-port-range 11000-11010 ...` will restrict the
validator to ports 11000-11011.

### Limiting ledger size to conserve disk space
By default the validator will retain the full ledger.  To conserve disk space
start the validator with the `--limit-ledger-size`, which will instruct the
validator to only retain the last couple hours of ledger.
