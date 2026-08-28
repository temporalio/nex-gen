//! Scoped generation settings shared across the compiler.
//!
//! Configuration is thread-local so concurrent in-process callers cannot affect
//! one another. Scopes nest and restore the prior configuration during unwinding.

use std::cell::RefCell;

use crate::generator::GenerationMode;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NexgenConfig {
    pub mode: GenerationMode,
    pub system_nexus: bool,
}

thread_local! {
    static CONFIGS: RefCell<Vec<NexgenConfig>> = RefCell::new(Vec::new());
}

pub fn current() -> NexgenConfig {
    CONFIGS.with(|configs| configs.borrow().last().copied().unwrap_or_default())
}

pub fn is_scoped() -> bool {
    CONFIGS.with(|configs| !configs.borrow().is_empty())
}

pub fn with_nexgen_config<T>(config: NexgenConfig, f: impl FnOnce() -> T) -> T {
    CONFIGS.with(|configs| configs.borrow_mut().push(config));
    let _scope = Scope;
    f()
}

struct Scope;

impl Drop for Scope {
    fn drop(&mut self) {
        CONFIGS.with(|configs| {
            configs
                .borrow_mut()
                .pop()
                .expect("NexGen configuration scope must be present");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{NexgenConfig, current, with_nexgen_config};
    use crate::generator::GenerationMode;

    #[test]
    fn restores_nested_configuration_after_unwinding() {
        let outer = NexgenConfig {
            mode: GenerationMode::DefinitionsOnly,
            system_nexus: true,
        };
        with_nexgen_config(outer, || {
            assert_eq!(current(), outer);
            let result = std::panic::catch_unwind(|| {
                with_nexgen_config(NexgenConfig::default(), || panic!("test unwind"));
            });
            assert!(result.is_err());
            assert_eq!(current(), outer);
        });
        assert_eq!(current(), NexgenConfig::default());
    }

    #[test]
    fn is_isolated_per_thread() {
        let config = NexgenConfig {
            mode: GenerationMode::DefinitionsOnly,
            system_nexus: true,
        };
        with_nexgen_config(config, || {
            assert_eq!(current(), config);
            assert_eq!(
                std::thread::spawn(current).join().unwrap(),
                NexgenConfig::default()
            );
        });
    }
}
