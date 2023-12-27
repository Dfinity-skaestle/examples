use candid::Principal;
use ic_cdk::api::management_canister::{
    main::{
        create_canister, install_code, update_settings, CanisterInstallMode,
        CreateCanisterArgument, InstallCodeArgument, UpdateSettingsArgument,
    },
    provisional::CanisterSettings,
};

const NUM_LEDGERS: usize = 2;
// Inline wasm binary of data partition canister
pub const WASM: &[u8] =
    include_bytes!("../../target/wasm32-unknown-unknown/release/ledger_token_1.wasm.gz");

pub(crate) async fn create_ledgers_from_wasm() -> Vec<Principal> {
    let create_args = CreateCanisterArgument {
        settings: Some(CanisterSettings {
            controllers: Some(vec![ic_cdk::id()]),
            compute_allocation: Some(0.into()),
            memory_allocation: Some(0.into()),
            freezing_threshold: Some(0.into()),
        }),
    };

    let mut canister_ids = vec![];
    for i in 0..NUM_LEDGERS {
        let canister_record = create_canister(create_args.clone()).await.unwrap();
        let canister_id = canister_record.0.canister_id;

        // Make the canister controller of itself
        update_settings(UpdateSettingsArgument {
            canister_id,
            settings: CanisterSettings {
                controllers: Some(vec![ic_cdk::id(), canister_id]),
                compute_allocation: Some(0.into()),
                memory_allocation: Some(0.into()),
                freezing_threshold: Some(0.into()),
            },
        })
        .await
        .unwrap();

        ic_cdk::println!("Created canister {}", canister_id);

        let install_args = InstallCodeArgument {
            mode: CanisterInstallMode::Install,
            canister_id,
            wasm_module: WASM.to_vec(),
            arg: vec![],
        };

        install_code(install_args).await.unwrap();

        // XXX - Make TokenName a shared type def between both canisters.
        let token_names: Vec<String> = if i == 0 {
            vec!["ICP".to_string()]
        } else {
            vec!["USD".to_string(), "EUR".to_string()]
        };
        let token_balances: Vec<u64> = if i == 0 {
            vec![10000]
        } else {
            vec![10000, 10000]
        };

        let _: () = ic_cdk::call(canister_id, "init", (token_names, token_balances))
            .await
            .unwrap();

        canister_ids.push(canister_id);

        // Adding the canister itself as a controller.
    }

    canister_ids
}
