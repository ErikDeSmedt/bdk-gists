use std::str::FromStr;

use bdk_testenv::bitcoincore_rpc::{Client, RpcApi};
use bdk_wallet::bitcoin::Network;
use bdk_wallet::{CreateParams, KeychainKind, PersistedWallet, SignOptions};

use bdk_testenv::TestEnv;
use bitcoin::Amount;

use rusqlite;


fn generate(client: &Client, num_block: u64) -> () {
    let address = client.get_new_address(None, None).expect("Failed to create address");
    client.generate_to_address(num_block, address.assume_checked_ref()).expect("Failed to mine block");
}

#[test]
fn cancel_tx_on_persisted_wallet() {
    // Create a test environment and mine initial funds
    let env = TestEnv::new().expect("Failed to launch bitcoind");
    let client = env.bitcoind.create_wallet("").expect("Failed to create wallet");
    generate(&client, 106);

    let mut conn = rusqlite::Connection::open(":memory:").unwrap();
    // Create the wallet under test
    let mut wallet = {
        const DESC: &str ="tr(tprv8ZgxMBicQKsPdnxnfzsvrUZ58eNTq85PA6nhJALQiGy9GVhcvXmHX2r9znpyApMVNLdkPBp3WArLgU3UnA6npK9TtGoZDKdAjjkoYm3rY7F/84'/0'/0'/0/*)";
        let create_params = CreateParams::new_single(DESC).network(Network::Regtest);
        PersistedWallet::create(
            &mut conn,
            create_params
        )
    }
    .expect("Wallet can be created");

    // Fund the wallet
    let address = wallet.reveal_next_address(KeychainKind::External).address;
    let funding_txid = client.send_to_address(&address, Amount::from_sat(200_000), None, None, None, None, None, None).unwrap();
    let funding_tx = client.get_raw_transaction(&funding_txid, None).unwrap();
    wallet.apply_unconfirmed_txs([(funding_tx.clone(), 100)]);

    // Spend the funding transaction
    let addr = bitcoin::Address::from_str("bcrt1pjsrxx204clmlkd05ssxgw99ehndlmr5m80s6s7zxy2cykx0jdd0qx04urr").unwrap().assume_checked();
    let mut builder = wallet.build_tx();
    builder.add_recipient(addr.script_pubkey(), Amount::from_sat(100_000));
    let mut psbt = builder.finish().unwrap();
    wallet.sign(&mut psbt, SignOptions::default()).unwrap();
    let spending_tx = psbt.extract_tx().unwrap();
    let spending_txid = spending_tx.compute_txid();

    wallet.apply_unconfirmed_txs([(spending_tx.clone(), 101)]);

    println!("Funding tx: {}", funding_txid);
    println!("Cancel tx: {}", spending_txid);

    // Persist the wallet
    wallet.persist(&mut conn).unwrap();

    // Verify that we have a single unspent output. 
    // This is the change coming from `tx`
    let outputs = wallet.list_unspent().map(|o| o.outpoint).collect::<Vec<_>>();
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].txid, spending_tx.compute_txid());
    wallet.persist(&mut conn).unwrap();

    // Cancel tx and verify that we now have funds from funding_tx
    println!("Outputs before cancel: {:?}", outputs);
    println!("Cancelling {}", spending_tx.compute_txid());
    wallet.cancel_tx(&spending_tx);


    // I expect that cancelling the `spending_tx`
    // will remove `spending_tx` from list_unspent
    // and that `funding_tx` will appear in `list-unspent`
    //
    // However, the `cancel_tx` operation has no effect
    let outputs = wallet.list_unspent().map(|o| o.outpoint).collect::<Vec<_>>();
    println!("Outputs after cancel: {:?}", outputs);
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].txid, funding_tx.compute_txid())
}

