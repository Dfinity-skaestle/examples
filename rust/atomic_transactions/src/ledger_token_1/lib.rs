use ic_cdk_macros::{query, update};
use ic_stable_structures::{BTreeMap, DefaultMemoryImpl};
use std::cell::RefCell;

#[update]
fn swap() {
    ic_cdk::println!("Swapping");
}
