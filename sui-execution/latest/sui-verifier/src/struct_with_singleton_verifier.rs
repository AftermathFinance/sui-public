// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This verifier ensures that types with the `singleton` ability are properly handled.
//! A singleton type can be instantiated at most once and only during the module's initialization.
//!
//! # Key properties enforced:
//! - Types with the singleton ability can only be instantiated in a module's `init` function.
//! - For each struct with the singleton ability, at most one of that type can be instantiated.
//! - Types with the singleton ability cannot have the `copy` ability.

use move_binary_format::file_format::{Bytecode, CompiledModule, StructDefinition};
use sui_types::error::ExecutionError;
use std::collections::HashMap;

use crate::{verification_failure, INIT_FN_NAME};

/// Verifies that all singleton types in the module follow singleton rules
pub fn verify_module(module: &CompiledModule) -> Result<(), ExecutionError> {
    let singletons = get_singletons(module);
    if singletons.is_empty() {
        return Ok(());
    }

    verify_singleton_constraints(module, &singletons).map_err(verification_failure)
}

/// Collects all struct types that have the singleton ability
fn get_singletons(module: &CompiledModule) -> Vec<(String, StructDefinition)> {
    module
        .struct_defs
        .iter()
        .filter_map(|def| {
            let handle = module.datatype_handle_at(def.struct_handle);
            if handle.abilities.has_singleton() {
                let name = module.identifier_at(handle.name).to_string();
                Some((name, def.clone()))
            } else {
                None
            }
        })
        .collect()
}

/// Verifies that singleton types are only instantiated in init and at most once and do not
/// have the `copy` ability.
fn verify_singleton_constraints(
    module: &CompiledModule,
    singletons: &[(String, StructDefinition)],
) -> Result<(), String> {
    // Track Pack operations for each singleton type
    let mut pack_counts: HashMap<&str, usize> =
        singletons.iter().map(|(name, _)| (name.as_str(), 0)).collect();

    verify_no_singleton_has_the_copy_ability(module, singletons)?;

    for fn_def in &module.function_defs {
        let fn_handle = module.function_handle_at(fn_def.function);
        let is_init = module.identifier_at(fn_handle.name) == INIT_FN_NAME;

        if let Some(code) = &fn_def.code {
            verify_function_bytecode(module, singletons, &mut pack_counts, code, is_init)?;
        }
    }

    Ok(())
}

fn verify_no_singleton_has_the_copy_ability(
    module: &CompiledModule,
    singleton_structs: &[(String, StructDefinition)],
) -> Result<(), String> {
    singleton_structs
        .iter()
        .find(|(name, def)| module.datatype_handle_at(def.struct_handle).abilities.has_copy())
        .map_or(Ok(()), |(name, _)| {
            Err(format!(
                "Singleton type {}::{} cannot have the copy ability",
                module.self_id(),
                name
            ))
        })
}

/// Verifies Pack operations for singleton types occur only in the init function and at most once
fn verify_function_bytecode(
    module: &CompiledModule,
    singletons: &[(String, StructDefinition)],
    pack_counts: &mut HashMap<&str, usize>,
    code: &move_binary_format::file_format::CodeUnit,
    is_init: bool,
) -> Result<(), String> {
    for bcode in &code.code {
        if let Bytecode::Pack(idx) = bcode {
            let packed_def = module.struct_def_at(*idx);

            // Check each singleton type
            for (name, def) in singletons {
                if packed_def == def {
                    // Verify Pack location
                    if !is_init {
                        return Err(format!(
                            "Singleton type {}::{} can only be instantiated in the init function",
                            module.self_id(),
                            name
                        ));
                    }

                    // Update and verify count
                    let count = pack_counts.get_mut(name.as_str()).unwrap();
                    *count += 1;
                    if *count > 1 {
                        return Err(format!(
                            "Singleton type {}::{} can be instantiated at most once",
                            module.self_id(),
                            name,
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}
