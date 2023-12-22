#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Address, token, panic_with_error, symbol_short, Symbol};

use crate::types::*;

mod types;

#[contract]
pub struct AuctionContract;

#[contractimpl]
impl AuctionContract {

    /// Setup a reverse Dutch Auction for a specified asset.
    /// The auction host specifies what asset they want to buy, and in what asset they will pay.
    /// The starting bid is set, and it will increase as per the provided parameters.
    /// The bid will increase every X ledgers, by Y amount, for a maximum of Z times.
    /// When a party holding the asset is willing to sell for that price, they call sell_asset and receive the price of that moment.
    pub fn setup_auction(env: Env, host: Address,
                         auction_asset: Address, 
                         counter_asset: Address, 
                         starting_bid: i128, 
                         bid_incr_amount:i128, 
                         bid_incr_times:u32, 
                         bid_incr_interval: u32)
                          -> Result<Status, Error> {
        

        // Require auth for the host of the auction, as it will pay for the asset it wants to buy
        host.require_auth();

        // Panic if the auction already has a State (Running, Fulfilled, Closed or Aborted)
        if  env.storage().instance().has(&DataKey::State) {
            return Ok(Status::AuctionAlreadyInitialised);
        }

        // Check if the starting bid and bid-increase amount are positive
        if starting_bid < 1 || bid_incr_amount < 1 {
            return Ok(Status::BidMustBePositive)
        }
        
        // Bump the instance to ~ max auction duration + a bit more
        let required_ttl: u32 = bid_incr_times * bid_incr_interval;
        env.storage().instance().extend_ttl(required_ttl, required_ttl + 1000);

        // Transfer enough counter-asset from the host to the contract to pay out the maximum prize.
        // The auction holds that balance until it is either Fullfilled or Aborted
        let max_increase: i128 = bid_incr_amount * bid_incr_times as i128;
        let max_price: i128 = starting_bid + max_increase;
        let transfer = token::Client::new(&env, &counter_asset)
                                        .try_transfer(&host, &env.current_contract_address(), &max_price);
        if transfer.is_err() {
            return Ok(Status::TransferError)
        }
        
        // Set auction details into storage
        let new_auction_data = AuctionData {
            host: host,
            asset: auction_asset,
            counter_asset: counter_asset,
            auction_start_ledger: env.ledger().sequence(),
            bid_start_amount: starting_bid,
            bid_incr_amount: bid_incr_amount,
            bid_incr_interval: bid_incr_interval,
            bid_incr_times: bid_incr_times,
            bid_max_amount: max_price
        };
        env.storage().instance().set(&DataKey::AuctionData, &new_auction_data);

        // Set the State to Running
        env.storage().instance().set(&DataKey::State, &State::Running);

        // Emit an event with the auction data, so stakeholders can calculate bid information off-chain
        env.events().publish((Symbol::new(&env, "auction_data"),), new_auction_data);

        // Return the AuctionStarted status
        Ok(Status::AuctionStarted)

    }

    /// Return the current price/bid that will be paid for the asset, upcoming changes and the maximum bid for the asset
    pub fn get_bid_info(env: Env, ) -> Result<BidInfo, Error> {
        
        // You can only query the price if the auction is Running
        if  !env.storage().instance().has(&DataKey::State) {
            panic_with_error!(&env, Error::AuctionNotInitialised);
        } else {
            let auction_state: State = env.storage().instance().get(&DataKey::State).unwrap();

            if auction_state != State::Running {
                panic_with_error!(&env, Error::AuctionNotRunning);
              }
        }
        // Retrieve the auction data & current ledger
        let current_ledger = env.ledger().sequence();
        let auction_data: AuctionData = env.storage().instance().get(&DataKey::AuctionData).unwrap();

        let starting_price: i128 = auction_data.bid_start_amount;
        let starting_ledger: u32 = auction_data.auction_start_ledger;
        let increase_amount: i128 = auction_data.bid_incr_amount;
        let increase_interval: u32 = auction_data.bid_incr_interval;
        let increase_times: u32 = auction_data.bid_incr_times;

        // Calculate the bid information
        let max_bid_ledger = starting_ledger + (increase_interval * increase_times);
        let max_bid = auction_data.bid_max_amount;

        // Declare these variables, so they can be set inside the if scope, then read outside it
        let current_bid: i128;
        let next_bid: i128;
        let next_bid_ledger: u32;
        let ledgers_to_next_increase: u32;

        // If the bid has reached its maximum, report that maximum as upcoming bid
        if current_ledger >= max_bid_ledger {
            current_bid = max_bid;
            next_bid = max_bid;
            next_bid_ledger = 0;
            ledgers_to_next_increase = 0;
        } 
        // If the bid is not yet at it's maximum, report the current and upcoming price/bid info
        else {
            let ledgers_passed = current_ledger - starting_ledger;
            let times_increased: u32 = ledgers_passed / increase_interval;

            current_bid = starting_price + (increase_amount * times_increased as i128);
            next_bid = current_bid + increase_amount;
            next_bid_ledger = starting_ledger + ((times_increased + 1) * increase_interval);
            ledgers_to_next_increase = next_bid_ledger - current_ledger;
        }

        let new_bid_info: BidInfo = BidInfo {
            current_bid,
            current_ledger,
            ledgers_to_next_increase,
            max_bid,
            max_bid_ledger,
            next_bid,
            next_bid_ledger,
        };

        // Publish an event with the bid information, so others can get the information without invoking the contract 
        env.events().publish((symbol_short!("bid_info"),), new_bid_info);
        return Ok(new_bid_info)

    }


    /// A holder of the asset that is being bid for can sell it, and receive the current bid for it
    pub fn sell_asset(env: Env, seller: Address ) -> Result<Status, Error> {

        // You can only sell the asset if the auction is Running
        if  !env.storage().instance().has(&DataKey::State) {
            return Ok(Status::AuctionNotInitialised);
        } else {
            let auction_state: State = env.storage().instance().get(&DataKey::State).unwrap();

            if auction_state != State::Running {
                return Ok(Status::AuctionNotRunning);
            }
        }

        // The seller needs to be authorised, since it will transfer the asset to the contract
        seller.require_auth();

        // Retrieve the auction data to read the asset and counter_asset data
        let auction_data: AuctionData = env.storage().instance().get(&DataKey::AuctionData).unwrap();

        let current_price = Self::get_bid_info(env.clone()).unwrap().current_bid;

        let auction_asset: Address = auction_data.asset;
        let counter_asset: Address = auction_data.counter_asset;
        
        // The amount of the auction asset is currently hardcoded to 1 stroop (NFT)
        // Transfer that 1 stroop from the seller to the contract
        let transfer = token::Client::new(&env, &auction_asset)
                                                .try_transfer(&seller, &env.current_contract_address(), &1);
        if transfer.is_err() {
            return Ok(Status::TransferError)
        }
        // Pay the seller the current bid/price
        let transfer = token::Client::new(&env, &counter_asset)
                                                .try_transfer(&env.current_contract_address(), &seller, &current_price);
        if transfer.is_err() {
            return Ok(Status::TransferError)
        }

        // Set the auction State to Fulfilled
        env.storage().instance().set(&DataKey::State, &State::Fulfilled);

        // Publish the fact the auction is fulfilled, and the current price
        env.events().publish((symbol_short!("fulfilled"),), current_price);

        // Return the AuctionFulfilled state to the seller
        Ok(Status::AuctionFulfilled)
    }

    /// The auction host/organiser can close the auction.
    /// If this is done while the auction is still running, they receive back the funds they deposited.
    /// If it is done after the auction was fulfilled, they receive the asset in question, and any remaining funds
    pub fn close_auction(env: Env,) -> Result<Status, Error> {

        // Only allow termination if the auction is either running or finished
        if  !env.storage().instance().has(&DataKey::State) {
            return Ok(Status::AuctionNotInitialised);
        } else {
            let auction_state: State = env.storage().instance().get(&DataKey::State).unwrap();

            if auction_state == State::Closed || auction_state == State::Aborted{
                return Ok(Status::AuctionAlreadyClosed);
            }
        }
        
        // Load the auction data
        let auction_data: AuctionData = env.storage().instance().get(&DataKey::AuctionData).unwrap();
        let host: Address = auction_data.host;
        let counter_asset: Address = auction_data.counter_asset;

        // Only the host of the auction can terminate it
        host.require_auth();

        let counter_asset_balance = token::Client::new(&env, &counter_asset).balance(&env.current_contract_address());
        
        let auction_state: State = env.storage().instance().get(&DataKey::State).unwrap();

        
        if auction_state == State::Running {
            // Auction is running, so pay the counter_asset back to the host and set status to Aborted
            let transfer = token::Client::new(&env, &counter_asset)
                                            .try_transfer(&env.current_contract_address(), &host, &counter_asset_balance);
            if transfer.is_err() {
                return Ok(Status::TransferError)
            }
            env.storage().instance().set(&DataKey::State, &State::Aborted);
            return Ok(Status::AuctionAborted)

        } else if auction_state == State::Fulfilled {
            // Auction is Fulfilled, so pay out the aquired asset
            let auction_asset: Address = auction_data.asset;
            let auction_asset_balance: i128 = token::Client::new(&env, &auction_asset).balance(&env.current_contract_address());

            let transfer = token::Client::new(&env, &auction_asset)
                                            .try_transfer(&env.current_contract_address(), &host, &auction_asset_balance);
            if transfer.is_err() {
                return Ok(Status::TransferError)
            }

            // If any funds remain, return them to the host
            if counter_asset_balance > 0 {
                let transfer = token::Client::new(&env, &counter_asset)
                                            .try_transfer(&env.current_contract_address(), &host, &counter_asset_balance);
                if transfer.is_err() {
                    return Ok(Status::TransferError)
                }
            }

            // Set the State to Closed
            env.storage().instance().set(&DataKey::State, &State::Closed);

            // Return the AuctionClosed status
            return Ok(Status::AuctionClosed)

        } else {
            // Its not Running or Fulfilled, so it must be Aborted or Closed already
            return Err(Error::AuctionNotRunning)

        }

    }

    /// For demonstration purposes, the host can reset the auction contract.
    /// This allows for re-use of the contract after it has been Closed or Aborted
    pub fn reset_auction(env: Env,) -> Result<Status, Error> {
        if  !env.storage().instance().has(&DataKey::State) {
            return Err(Error::AuctionNotInitialised)
        }
        let auction_data: AuctionData = env.storage().instance().get(&DataKey::AuctionData).unwrap();
        auction_data.host.require_auth();

        let auction_state: State = env.storage().instance().get(&DataKey::State).unwrap();
        if auction_state == State::Closed || auction_state == State::Aborted {
            env.storage().instance().remove(&DataKey::State);
            env.storage().instance().remove(&DataKey::AuctionData);
            
            return Ok(Status::AuctionReset);
        }
        else {
            return Err(Error::AuctionNotYetClosed);
        }
        
    }
}

#[cfg(test)]
mod test;