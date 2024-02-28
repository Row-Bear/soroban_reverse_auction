extern crate std;
use core::cmp::min;
use std::println;

use crate::{AuctionContract, AuctionContractClient, token, types::Status};
use soroban_sdk::{Env, testutils::Address as _, Address, testutils::Ledger as Ledger};


fn create_token_contract<'a>(e: &Env, admin: &Address) -> token::StellarAssetClient<'a> {
    token::StellarAssetClient::new(e, &e.register_stellar_asset_contract(admin.clone()))
}

fn calculate_bid (env: &Env,
                 start_bid: i128,
                 start_ledger: u32, 
                 incr_amount: i128, 
                 incr_times: u32 , 
                 incr_interval: u32
                ) -> i128 {
    let current_ledger = env.ledger().sequence();
    let ledgers_passed = current_ledger - start_ledger;
    let times_increased = min(ledgers_passed / incr_interval, incr_times);
    
    let current_bid = start_bid + (incr_amount * times_increased as i128);

    current_bid
}


#[test]
fn test_setup(){
    let env = Env::default();
    env.mock_all_auths();

    let sell_after_ledger: u32 = 567;
    
    // Generate two assets to use in the Auction
    let host = Address::generate(&env);
    let seller = Address::generate(&env);

    // Create and provide the seller with an NFT to sell
    let asset_issuer = Address::generate(&env);
    let asset = create_token_contract(&env, &asset_issuer);
    let asset_token = token::Client::new(&env, &asset.address);
    
    asset.mint(&seller, &1);
    assert_eq!(asset_token.balance(&seller), 1);
    assert_eq!(asset_token.balance(&host), 0);

    // Create and distribute counter-asset
    let counter_asset_issuer = Address::generate(&env);
    let counter_asset = create_token_contract(&env, &counter_asset_issuer);    
    let counter_asset_token = token::Client::new(&env, &counter_asset.address);

    counter_asset.mint(&host, &100_000_000);
    assert_eq!(counter_asset_token.balance(&seller), 0);
    assert_eq!(counter_asset_token.balance(&host), 100_000_000);
    println!("Assets generated and distributed. Starting contract tests.");

    // Generate an auction contract
    let contract_id = env.register_contract(None, AuctionContract);
    let auction_client = AuctionContractClient::new(&env, &contract_id);

    let test_starting_bid = 1000;
    let test_bid_incr_amount = 100;
    let test_bid_incr_times:u32 = 10;
    let test_bid_incr_interval: u32 = 100;
    
    let auction_start_ledger = env.ledger().sequence();

    // Set up the auction with sensible values
    let test_setup = auction_client.setup_auction(&host, 
        &asset.address,
        &counter_asset.address, 
        &test_starting_bid, 
        &test_bid_incr_amount, 
        &test_bid_incr_times,
        &test_bid_incr_interval);
    
    let test_max_bid = test_starting_bid + (test_bid_incr_amount * test_bid_incr_times as i128);

    assert_eq!(test_setup, Status::Started);
    assert_eq!(counter_asset_token.balance(&contract_id), test_max_bid);

    // Attempt to set up the auction again, expected to fail since the auction is already running
    let test_setup = auction_client.setup_auction(&host, 
        &asset.address,
        &counter_asset.address, 
        &10, 
        &10, 
        &10,
        &10);
    assert_eq!(test_setup, Status::AlreadyInitialised);
    
    println!("Auction created.");

    // Check if get_bid_info returns the expected value for the current bid.
    let mut test_get_bid_info = auction_client.get_bid_info();
    let mut current_bid = calculate_bid(&env,
        test_starting_bid,
        auction_start_ledger,
        test_bid_incr_amount,
        test_bid_incr_times,
        test_bid_incr_interval);
    assert_eq!(current_bid, test_get_bid_info.current_bid);

    println!("Bid info verified: bid is {} at ledger {}", current_bid, env.ledger().sequence());

    // Advance the ledger sequence as many times as we increase the price. 
    // Check the price information is as expected.
    // Then advance two more times, beyond the maximum and check the price
    for _ in 0..test_bid_incr_times + 2 {
        // Advance the ledger up to 1 ledger before price increase
        env.ledger().with_mut(|li|li.sequence_number += test_bid_incr_interval -1 );

        test_get_bid_info = auction_client.get_bid_info();
        current_bid = calculate_bid(&env,
            test_starting_bid,
            auction_start_ledger,
            test_bid_incr_amount,
            test_bid_incr_times,
            test_bid_incr_interval);
        assert_eq!(current_bid, test_get_bid_info.current_bid);

        // Advance the ledger 1 more, to the ledger of price increase
        env.ledger().with_mut(|li|li.sequence_number += 1 );

        test_get_bid_info = auction_client.get_bid_info();
        current_bid = calculate_bid(&env,
            test_starting_bid,
            auction_start_ledger,
            test_bid_incr_amount,
            test_bid_incr_times,
            test_bid_incr_interval);
        assert_eq!(current_bid, test_get_bid_info.current_bid);
        println!("Bid info verified: bid is {} at ledger {}", current_bid, env.ledger().sequence());
    }
    println!("");
    // Revert the ledger sequence back to 0, to allow testing the sale at specific ledgers
    env.ledger().with_mut(|li|li.sequence_number = 0);

    // Set the ledger to a desired ledger sequence for testing the sale
    env.ledger().with_mut(|li|li.sequence_number = sell_after_ledger);

    current_bid = calculate_bid(&env,
        test_starting_bid,
        auction_start_ledger,
        test_bid_incr_amount,
        test_bid_incr_times,
        test_bid_incr_interval);

    println!("Ledger sequence reset to {} to test sell_asset function.", env.ledger().sequence());
    println!("Preparing to sell asset at ledger {}.", env.ledger().sequence());
    println!("The seller has {} of the auction asset and {} of the counter-asset.", asset_token.balance(&seller), counter_asset_token.balance(&seller));
    println!("The contract has {} of the auction asset and {} of the counter-asset.", asset_token.balance(&contract_id), counter_asset_token.balance(&contract_id));

    // Sell the asset to the auction and verify the correct status is returned. Log the sell price
    let test_sell = auction_client.sell_token(&seller);
    assert_eq!(test_sell, Status::Fulfilled);

    let sell_price = current_bid;
    
    // Check that seller has one less asset, and that it has gained the sell price
    assert_eq!(asset_token.balance(&seller), 0);
    assert_eq!(counter_asset_token.balance(&seller), sell_price);

    // Check that the contract has received the asset, and that the host has not (yet).
    // Check that the contracts balance of the couter-asset matches expectation
    assert_eq!(asset_token.balance(&contract_id), 1);
    assert_eq!(asset_token.balance(&host), 0);
    assert_eq!(counter_asset_token.balance(&contract_id), (test_starting_bid + (test_bid_incr_amount * test_bid_incr_times as i128)) - sell_price);
    println!("Sold the asset at ledger {} for {}. ", env.ledger().sequence(), sell_price);
    println!("The seller now has {} of the auction asset and {} of the counter-asset.",  asset_token.balance(&seller), counter_asset_token.balance(&seller));
    println!("The contract now has {} of the auction asset and {} of the counter-asset.", asset_token.balance(&contract_id), counter_asset_token.balance(&contract_id));
    println!("");
    
    // Try to iniate sale again
    let test_sell = auction_client.sell_token(&seller);
    assert_eq!(test_sell, Status::NotRunning);

    // Check if the balances are not changed
    assert_eq!(asset_token.balance(&seller), 0);
    assert_eq!(counter_asset_token.balance(&seller), sell_price);
    assert_eq!(asset_token.balance(&contract_id), 1);
    assert_eq!(asset_token.balance(&host), 0);
    assert_eq!(counter_asset_token.balance(&contract_id), test_max_bid - sell_price);

    println!("Preparing to close the auction");
    println!("The host has {} of the auction asset and {} of the counter-asset.", asset_token.balance(&host), counter_asset_token.balance(&host));
    println!("");

    // Attempt to close the auction
    let test_close = auction_client.close_auction();
    assert_eq!(test_close, Status::Closed);

    // Verify the balances are as expected: Seller balances unchanged, contract balances reduces, host balances increased
    assert_eq!(asset_token.balance(&seller), 0);
    assert_eq!(asset_token.balance(&contract_id), 0);
    assert_eq!(asset_token.balance(&host), 1);
    assert_eq!(counter_asset_token.balance(&seller), sell_price);
    assert_eq!(counter_asset_token.balance(&contract_id), 0);
    assert_eq!(counter_asset_token.balance(&host), 100_000_000 - sell_price);

    println!("Closed the auction");
    println!("The seller now has {} of the auction asset and {} of the counter-asset.",  asset_token.balance(&seller), counter_asset_token.balance(&seller));
    println!("The contract now has {} of the auction asset and {} of the counter-asset.", asset_token.balance(&contract_id), counter_asset_token.balance(&contract_id));
    println!("The host now has {} of the auction asset and {} of the counter-asset.", asset_token.balance(&host), counter_asset_token.balance(&host));

    println!("");

    // Attempt to close the auction a second time
    let test_close = auction_client.close_auction();
    assert_eq!(test_close, Status::AlreadyClosed);
    
    // Verify the balances have not changed
    assert_eq!(asset_token.balance(&seller), 0);
    assert_eq!(asset_token.balance(&contract_id), 0);
    assert_eq!(asset_token.balance(&host), 1);
    assert_eq!(counter_asset_token.balance(&seller), sell_price);
    assert_eq!(counter_asset_token.balance(&contract_id), 0);
    assert_eq!(counter_asset_token.balance(&host), 100_000_000 - sell_price);

    println!("Tests completed!");
    println!("");
}

#[test]
fn i128() {

    let i:i128 = 1000;
    let j:i128 = 100;

    println!("First: {} has hi {} and lo {}", i, (i >> 64) as i64, i as u64);
    println!("Second: {} has hi {} and lo {}", j, (j >> 64) as i64, j as u64);

}
