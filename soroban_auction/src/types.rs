use soroban_sdk::{contracttype, Address, contracterror};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Data {
    pub buyer: Address,
    pub token: Address,
    pub counter_token: Address,
    pub auction_start_ledger: u32,
    pub bid_start_amount: i128,
    pub bid_incr_amount: i128,
    pub bid_incr_interval: u32,
    pub bid_incr_times: u32,
    pub bid_max_amount: i128
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct BidInfo {
    pub current_bid: i128,
    pub current_ledger: u32,
    pub ledgers_to_next_increase: u32,
    pub max_bid: i128,
    pub max_bid_ledger: u32,
    pub next_bid: i128,
    pub next_bid_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum State {
    Running,
    Fulfilled,
    Closed,
    Aborted,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DataKey {
    State,
    Data,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Status {
    AlreadyInitialised,
    AlreadyClosed,
    Started,
    Aborted,
    Closed,
    Fulfilled,
    NotInitialised,
    NotRunning,
    BidMustBePositive, 
    TransferError,   
    Reset,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialised = 1,
    AlreadyIntitialised= 2,
    NotRunning = 3,
    NotYetClosed = 4,
}