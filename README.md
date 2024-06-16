# Damn Vulnerable Blockchain

This project is a part of my article series on threat modelling for blockchains [Layered threat model for Web3 applications part 1— distribution of responsibility](https://medium.com/@ishish222/layered-threat-model-for-web3-applications-part-1-distribution-of-responsibility-86ab91cb7f81). The goal of this project is to provide basis for discussing potential threats and vulnerabilities in various layers of blockchain networks functional stack. **This project should not be used for production purposes without mitigating all internal weaknesses and vulnerabilities**.

## Building

Simply build with cargo:

```bash
cargo build
```

This will build two targets, a wallet manager and the client node binary (wallet, node).

## Usage

### Managing wallets with wallet

Creating a wallet:

```bash
$ target/debug/wallet create test
Home dir: <$HOME>/.dvb
Creating new wallet
Please enter a password for the wallet
<password>
Created wallet: 0x68d9D11C0A4D67074b78B797e4AC5aB4a50D3408 in keystore <$HOME>/.dvb/test
```

Printing a wallet:

```bash
$ target/debug/wallet print test
Home dir: <$HOME>/.dvb
Please enter a password for the wallet
<password>
Wallet: 0x68d9D11C0A4D67074b78B797e4AC5aB4a50D3408
```

Printing a private key:

```bash
$ target/debug/wallet print-private-key test
Home dir: <$HOME>/.dvb
Please enter a password for the wallet
<password>
Private key: <private key>
```

### Starting a node

To start a node simply execute:

```bash
$ target/debug/node <difficulty>
```

E.g.:

```bash
$ target/debug/node 2
Reading commands from stdin
Starting the local mining task
Local node is listening on /ip4/127.0.0.1/tcp/39447
Local node is listening on /ip4/192.168.1.2/tcp/39447
```

The difficulty of the PoW task is determined by a parameter istead of the consensus layer for simplicity. You can select the difficulty to speed up / slow down the mining process.

Once new peers are detected by the mDNS layer they will be included in the peer list:

```bash
mDNS discovered a new peer: 12D3KooWAva78wYNXjTySaJMQLwkrwC4cJFF2xo7cTSJ4BKTD2qR
```

The node will send the updates on successfully mined blocks and pool updates to identified nodes. It will also account for updates coming from the other nodes.

### Node commands summary

You need to enter the commands into stdin. First enter the keyword, then the required parameters.

The node accepts the following commands on the stdin:
- start
- stop
- open
- get_balance
- send_dvb
- print_pool

#### start

The start command starts the mining process. The mining thread will receive a command to commence mining a new block which is created using the existing internal blockchain state, the transaction pool, the coinbase address and the difficulty:

```bash
start
Processing command: start
Building a block proposal
mining_task::Updating current_block
mining_task::Starting mining
Starting the mining for a new block
proof_of_work::start
proof_of_work::finish
mining_task: Mined block
Successfuly mined block: DvbBlock { header: DvbBlockHeader { coinbase: 0x68d9d11c0a4d67074b78b797e4ac5ab4a50d3408, number: 0, nonce: 14689956009786713665, difficulty: 2, cur_hash: [0, 0, 254, 204, 27, 221, 37, 126, 20, 156, 190, 13, 23, 125, 208, 159, 15, 31, 9, 5, 72, 128, 227, 158, 173, 144, 139, 244, 104, 192, 247, 34], prev_hash: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] }, content: [] }
Updated balance for 0x68d9D11C0A4D67074b78B797e4AC5aB4a50D3408: AccountInfo { balance: 1, nonce: 0, code_hash: 0xc5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470, code: Some(LegacyAnalyzed(LegacyAnalyzedBytecode { bytecode: 0x00, original_len: 0, jump_table: JumpTable { map: "00" } })) }
Building a block proposal
mining_task::Updating current_block
Starting the mining for a new block
[...]
```

Please note that you need to open a wallet before starting mining in order to set up the coinbase address.

#### stop

Stops the mining task.

```bash
stop
Processing command: stop
proof_of_work::finish
mining_task: Mined block
mining_task::Stopping mining
```

#### open

Opens the wallet as coinbase:

```bash
open
Processing command: open
Enter the name of the wallet [default]
test
Please enter a password for the wallet
<password>
Opening wallet: <$HOME>/.dvb/test
Opened wallet: 0x68d9D11C0A4D67074b78B797e4AC5aB4a50D3408
```

#### get_balance

Retrieves the balance from the current network state. The current network state is evaluated by processing all the transactions included in blocks so far. Internally it's represented by a Merkle tree via revm::db::InMemoryDB object.

In the current version of DVB the state only contains balance of the wallet.

```bash
get_balance
Processing command: get_balance
Enter the name of the wallet [coinbase]
0x68d9D11C0A4D67074b78B797e4AC5aB4a50D3408
Balance of 0x68d9D11C0A4D67074b78B797e4AC5aB4a50D3408: 2
```
#### send_dvb

Send dvb (native currency for DVB):

```bash
get_balance
Processing command: get_balance
Enter the name of the wallet [coinbase]

Balance of 0x93Ae5cf7A6eEa2F5D7144dd4E9Df025A692aAaD0: 7
get_balance 
Processing command: get_balance
Enter the name of the wallet [coinbase]
0x68d9D11C0A4D67074b78B797e4AC5aB4a50D3408
Balance of 0x68d9D11C0A4D67074b78B797e4AC5aB4a50D3408: 3
send_dvb
Processing command: send_dvb
Enter the name of the source wallet [coinbase]

Enter the target wallet
0x68d9D11C0A4D67074b78B797e4AC5aB4a50D3408
How much dvb to send?
5
Sending 5 dvb from 0x93Ae5cf7A6eEa2F5D7144dd4E9Df025A692aAaD0 to 0x68d9D11C0A4D67074b78B797e4AC5aB4a50D3408
Sending line: "TRA{\"from\":\"0x93ae5cf7a6eea2f5d7144dd4e9df025a692aaad0\",\"to\":\"0x68d9d11c0a4d67074b78b797e4ac5ab4a50d3408\",\"amount\":5}"
Transaction added to local pool
Current pool: [DvbTransaction { from: 0x93ae5cf7a6eea2f5d7144dd4e9df025a692aaad0, to: 0x68d9d11c0a4d67074b78b797e4ac5ab4a50d3408, amount: 5 }]
```

Once the new block is mined: 

```bash
Updated balance for 0x93Ae5cf7A6eEa2F5D7144dd4E9Df025A692aAaD0: 2
Updated balance for 0x68d9D11C0A4D67074b78B797e4AC5aB4a50D3408: 9
Removed transaction DvbTransaction { from: 0x93ae5cf7a6eea2f5d7144dd4e9df025a692aaad0, to: 0x68d9d11c0a4d67074b78b797e4ac5ab4a50d3408, amount: 5 } local pool
Current pool: []
```

#### print_pool

Prints the current local transaction pool:

```bash
print_pool
Processing command: print_pool
Current pool: [DvbTransaction { from: 0x93ae5cf7a6eea2f5d7144dd4e9df025a692aaad0, to: 0x68d9d11c0a4d67074b78b797e4ac5ab4a50d3408, amount: 5 }]
```

