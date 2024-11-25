use std::collections::HashMap;

use bdk_testenv::bitcoincore_rpc::json::CreateRawTransactionInput;
use bdk_testenv::bitcoincore_rpc::{Client, RawTx, RpcApi};
use bdk_bitcoind_rpc::Emitter;
use bdk_wallet::bitcoin::Network;
use bdk_wallet::{CreateParams, KeychainKind, Wallet};

use bdk_testenv::TestEnv;
use bitcoin::{Amount, Transaction};

fn generate(client: &Client, num_block: u64) -> () {
    let address = client.get_new_address(None, None).expect("Failed to create address");
    client.generate_to_address(num_block, address.assume_checked_ref()).expect("Failed to mine block");
}

#[test]
fn detect_double_spend() {
    // Create a test environment and mine initial funds
    let env = TestEnv::new().expect("Failed to launch bitcoind");
    let client = env.bitcoind.create_wallet("").expect("Failed to create wallet");
    generate(&client, 106);

    // Create the wallet under test
    let mut alice = {
        const DESC: &str ="tr(tprv8ZgxMBicQKsPdnxnfzsvrUZ58eNTq85PA6nhJALQiGy9GVhcvXmHX2r9znpyApMVNLdkPBp3WArLgU3UnA6npK9TtGoZDKdAjjkoYm3rY7F/84'/0'/0'/0/*)";
        Wallet::create_with_params(
            CreateParams::new_single(DESC).network(Network::Regtest)
        )
    }
    .expect("Wallet can be created");

    // Sync it with the chain
    let mut emitter = Emitter::new(&env.bitcoind.client, alice.latest_checkpoint(), 0);
    while let Some(ev) = emitter.next_block().unwrap() {
        alice.apply_block_connected_to(&ev.block, ev.block_height(), ev.connected_to()).unwrap();
    }

     // Creates some transactions
    let unspent = client.list_unspent(None, None, None, Some(false), None).unwrap();

    let input_amount = unspent[0].amount;
    let destination_amount = Amount::from_sat(100_000);
    let fee_amount = Amount::from_sat(2_000);
    let change_amount = input_amount - destination_amount - fee_amount;

    // Create a transaction that pays Alice
    let address = alice.reveal_next_address(KeychainKind::External).address;
    let change_address = client.get_new_address(None, None).unwrap().assume_checked();
    let inputs = [
        CreateRawTransactionInput {
            txid: unspent[0].txid,
            vout: unspent[0].vout,
            sequence: Some(0xFFFFFFFE),
        }
    ];
    let mut outputs = HashMap::new();
    outputs.insert(address.to_string(), destination_amount);
    outputs.insert(change_address.to_string(), change_amount);
    let tx1a = client.create_raw_transaction(&inputs, &outputs, None, None).unwrap();
    let tx1a = client.sign_raw_transaction_with_wallet(tx1a.raw_hex(), None, None).unwrap().transaction().unwrap();

    // Create a double-spent of tx1a
    let address = client.get_new_address(None, None).unwrap().assume_checked();
    let mut outputs = HashMap::new();
    outputs.insert(address.to_string(), destination_amount);
    outputs.insert(change_address.to_string(), change_amount);
    let tx1b = client.create_raw_transaction(&inputs, &outputs, None, None).unwrap();
    let tx1b : Transaction = client.sign_raw_transaction_with_wallet(tx1b.raw_hex(), None, None ).unwrap().transaction().unwrap();

    println!("Transactions");
    println!("tx1a: {}", tx1a.compute_txid());
    println!("tx1b: {}", tx1b.compute_txid());

    // Alice observes tx1a in the mempool
    alice.apply_unconfirmed_txs(vec![(tx1a.clone(), 100)]);
    println!("Mempool (tx1a)");
    println!("- alice.list_unspent(): {:?}", alice.list_unspent().map(|o| o.outpoint).collect::<Vec<_>>());
    println!("- alice.list_transaction(): {:?}", alice.transactions().map(|t| t.tx_node.txid).collect::<Vec<_>>());
    println!("- alice.list_transaction(): {:?}", alice.transactions().map(|t| t.chain_position).collect::<Vec<_>>());

    // A block is create
    // In this block tx1a is doublespent by tx1b
    println!("Create block");
    println!("tx1a is doublespent by tx1b");
    client.send_raw_transaction(tx1b.raw_hex()).unwrap();
    generate(&client, 6);

    // Apply the block to the w1
    let mut emitter = Emitter::new(&env.bitcoind.client, alice.latest_checkpoint(), 0);
    while let Some(ev) = emitter.next_block().unwrap() {
        alice.apply_block_connected_to(&ev.block, ev.block_height(), ev.connected_to()).unwrap();
    }

    println!("After doublespent");
    println!("- alice.list_unspent(): {:?}", alice.list_unspent().map(|o| o.outpoint).collect::<Vec<_>>());
    println!("- alice.list_transaction(): {:?}", alice.transactions().map(|t| t.tx_node.txid).collect::<Vec<_>>());
    println!("- alice.list_transaction(): {:?}", alice.transactions().map(|t| t.chain_position).collect::<Vec<_>>());
    println!("- alice.balance: {:?}", alice.balance());

    // We also add txb do the wallet
    alice.apply_unconfirmed_txs([(tx1b.clone(), 101)]);
    println!("After applying tx1b");
    println!("- alice.list_unspent(): {:?}", alice.list_unspent().map(|o| o.outpoint).collect::<Vec<_>>());
    println!("- alice.list_transaction(): {:?}", alice.transactions().map(|t| t.tx_node.txid).collect::<Vec<_>>());
    println!("- alice.list_transaction(): {:?}", alice.transactions().map(|t| t.chain_position).collect::<Vec<_>>());
    println!("- alice.balance: {:?}", alice.balance());

    // I expect list_unspent to be empty.
    // (tx1a) was double-spent
    assert_eq!(alice.list_unspent().collect::<Vec<_>>(), vec![]);
    assert_eq!(alice.transactions().collect::<Vec<_>>(), vec![])
 }
