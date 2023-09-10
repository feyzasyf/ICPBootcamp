use candid::{CandidType, Decode, Deserialize, Encode};
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{BoundedStorable, DefaultMemoryImpl, StableBTreeMap, Storable};
use std::collections::BTreeMap;
use std::{borrow::Cow, cell::RefCell};

type Memory = VirtualMemory<DefaultMemoryImpl>;
const MAX_VALUE_SIZE: u32 = 5000;

#[derive(CandidType)]
enum AuctionError {
    InvalidBid,
    ItemIsNotActive,
    NoSuchItem,
    AccessRejected,
    UpdateError,
}

#[derive(CandidType, Deserialize)]
struct Item {
    description: String,
    is_active: bool,
    owner: candid::Principal,
    new_owner: Option<candid::Principal>,
    bid_count: u32,
    bids: BTreeMap<candid::Principal, u64>,
}

#[derive(CandidType, Deserialize)]
struct CreateItem {
    description: String,
    is_active: bool,
}

impl Storable for Item {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }
    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

impl BoundedStorable for Item {
    const MAX_SIZE: u32 = MAX_VALUE_SIZE;
    const IS_FIXED_SIZE: bool = false;
}

//implement memory

thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>>= RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));

    static ITEMS_MAP: RefCell<StableBTreeMap<u64, Item, Memory>>= RefCell::new(StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0)))))
}

#[ic_cdk::query]
fn get_all_items() -> Option<BTreeMap<u64, Item>> {
    let mut map: BTreeMap<u64, Item> = BTreeMap::new();

    ITEMS_MAP.with(|p| {
        for (k, v) in p.borrow().iter() {
            map.insert(k, v);
        }
    });
    return Some(map);
}

#[ic_cdk::query]
fn get_item(key: u64) -> Option<Item> {
    ITEMS_MAP.with(|p| p.borrow().get(&key))
}

#[ic_cdk::query]
fn get_item_count() -> u64 {
    ITEMS_MAP.with(|p| p.borrow().len())
}

#[ic_cdk::query]
fn get_item_sold_for_the_most() -> Result<Option<Item>, AuctionError> {
    let mut highest_bid: u64 = 0;
    let mut highest_bid_item_key: u64 = 0;
    let item = ITEMS_MAP.with(|p| {
        for (k, v) in p.borrow().iter() {
            if v.new_owner == None {
                continue;
            }

            let last_bid_option: Option<(&candid::Principal, &u64)> = v.bids.last_key_value();
            let last_bid = match last_bid_option {
                Some(value) => value,
                None => return Err(AuctionError::NoSuchItem),
            };

            if last_bid.1 > &highest_bid {
                highest_bid = *last_bid.1;
                highest_bid_item_key = k;
            }
        }
        return Ok(p.borrow().get(&highest_bid_item_key));
    });
    return item;
}

#[ic_cdk::query]
fn get_item_bid_on_the_most() -> Option<Item> {
    let mut highest_length: u32 = 0;
    let mut highest_bid_length_item_key: u64 = 0;
    let item = ITEMS_MAP.with(|p| {
        for (k, v) in p.borrow().iter() {
            let length = v.bid_count;
            if length > highest_length {
                highest_length = length;
                highest_bid_length_item_key = k
            }
        }
        return p.borrow().get(&highest_bid_length_item_key);
    });
    return item;
}

#[ic_cdk::update]
fn create_item(key: u64, item: CreateItem) -> Option<Item> {
    let value = Item {
        description: item.description,
        is_active: item.is_active,
        owner: ic_cdk::caller(),
        bids: BTreeMap::new(),
        bid_count: 0,
        new_owner: None,
    };

    ITEMS_MAP.with(|p| p.borrow_mut().insert(key, value))
}

#[ic_cdk::update]
fn bid_item(key: u64, bid: u64) -> Result<(), AuctionError> {
    ITEMS_MAP.with(|p| {
        let bidding_item_opt: Option<Item> = p.borrow().get(&key);
        let mut bidding_item = match bidding_item_opt {
            Some(value) => value,
            None => return Err(AuctionError::NoSuchItem),
        };

        let caller = ic_cdk::caller();

        let last_bid_opt = bidding_item.bids.last_key_value();
        let last_bid = match last_bid_opt {
            Some(value) => value,
            None => return Err(AuctionError::NoSuchItem),
        };
        if bid <= *last_bid.1 {
            return Err(AuctionError::InvalidBid);
        }

        bidding_item.bids.insert(caller, bid);
        bidding_item.bid_count += 1;

        let result = p.borrow_mut().insert(key, bidding_item);
        match result {
            Some(_) => Ok(()),
            None => Err(AuctionError::UpdateError),
        }
    })
}

#[ic_cdk::update]
fn edit_item(key: u64, new_description: String) -> Result<(), AuctionError> {
    ITEMS_MAP.with(|p| {
        let old_item_opt = p.borrow().get(&key);
        let old_item = match old_item_opt {
            Some(value) => value,
            None => return Err(AuctionError::NoSuchItem),
        };
        if ic_cdk::caller() != old_item.owner {
            return Err(AuctionError::AccessRejected);
        };
        if old_item.is_active == false {
            return Err(AuctionError::ItemIsNotActive);
        }

        let value = Item {
            description: new_description,
            ..old_item
        };
        let result = p.borrow_mut().insert(key, value);
        match result {
            Some(_) => Ok(()),
            None => Err(AuctionError::UpdateError),
        }
    })
}

#[ic_cdk::update]
fn stop_item(key: u64) -> Result<(), AuctionError> {
    ITEMS_MAP.with(|p| {
        let item_opt = p.borrow().get(&key);
        let item = match item_opt {
            Some(value) => value,
            None => return Err(AuctionError::NoSuchItem),
        };
        if ic_cdk::caller() != item.owner {
            return Err(AuctionError::AccessRejected);
        };
        let last_bid_opt = item.bids.last_key_value();

        let last_bid = match last_bid_opt {
            Some(value) => value,
            None => return Err(AuctionError::NoSuchItem),
        };

        let value = Item {
            is_active: false,
            new_owner: Some(*last_bid.0),
            ..item
        };
        let result = p.borrow_mut().insert(key, value);
        match result {
            Some(_) => Ok(()),
            None => Err(AuctionError::UpdateError),
        }
    })
}
