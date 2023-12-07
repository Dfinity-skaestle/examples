use ic_cdk::api::management_canister::{
    main::{
        create_canister, install_code, CanisterInstallMode, CreateCanisterArgument,
        InstallCodeArgument,
    },
    provisional::CanisterSettings,
};

const NUM_LEDGERS: usize = 5;
// Inline wasm binary of data partition canister
pub const WASM: &[u8] =
    include_bytes!("../../target/wasm32-unknown-unknown/release/ledger_token_1.wasm.gz");

pub(crate) async fn create_ledgers_from_wasm() {
    let create_args = CreateCanisterArgument {
        settings: Some(CanisterSettings {
            controllers: Some(vec![ic_cdk::id()]),
            compute_allocation: Some(0.into()),
            memory_allocation: Some(0.into()),
            freezing_threshold: Some(0.into()),
        }),
    };

    for i in 0..NUM_LEDGERS {
        let canister_record = create_canister(create_args.clone()).await.unwrap();
        let canister_id = canister_record.0.canister_id;

        ic_cdk::println!("Created canister {}", canister_id);

        let install_args = InstallCodeArgument {
            mode: CanisterInstallMode::Install,
            canister_id,
            wasm_module: WASM.to_vec(),
            arg: vec![],
        };

        install_code(install_args).await.unwrap();

        let token_names: Vec<u32> = vec![(i + 1) as u32];
        let token_balances: Vec<u64> = vec![10000];
        let _: () = ic_cdk::call(canister_id, "init", (token_names, token_balances))
            .await
            .unwrap();

        CANISTER_IDS.with(|canister_ids| {
            let mut canister_ids = canister_ids.borrow_mut();
            canister_ids.push(canister_id);
        });
    }
}
