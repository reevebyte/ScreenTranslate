use screentrans_core::{AppError, ErrorCode, Result, ports::SecretStore};
use std::{collections::HashMap, sync::Mutex};

const SERVICE: &str = "io.github.reevebyte.screentranslate.v2";

#[derive(Default)]
pub struct SystemSecretStore;

impl SecretStore for SystemSecretStore {
    fn get(&self, name: &str) -> Result<Option<String>> {
        match keyring::Entry::new(SERVICE, name).and_then(|entry| entry.get_password()) {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(credentials_error()),
        }
    }
    fn set(&self, name: &str, secret: &str) -> Result<()> {
        keyring::Entry::new(SERVICE, name)
            .and_then(|entry| entry.set_password(secret))
            .map_err(|_| credentials_error())
    }
    fn delete(&self, name: &str) -> Result<()> {
        match keyring::Entry::new(SERVICE, name).and_then(|entry| entry.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(credentials_error()),
        }
    }
}

fn credentials_error() -> AppError {
    AppError::new(
        ErrorCode::Credentials,
        "系统凭据库不可用或已锁定；可以只在本次会话中使用密钥",
    )
    .action("providers")
}

struct SessionSecret {
    value: String,
    durable: bool,
}

pub struct CredentialVault {
    persistent: Box<dyn SecretStore>,
    session: Mutex<HashMap<String, SessionSecret>>,
}

impl Default for CredentialVault {
    fn default() -> Self {
        Self {
            persistent: Box::new(SystemSecretStore),
            session: Mutex::new(HashMap::new()),
        }
    }
}

impl CredentialVault {
    pub fn session_only() -> Self {
        struct SessionOnly;
        impl SecretStore for SessionOnly {
            fn get(&self, _name: &str) -> Result<Option<String>> {
                Ok(None)
            }
            fn set(&self, _name: &str, _secret: &str) -> Result<()> {
                Err(credentials_error())
            }
            fn delete(&self, _name: &str) -> Result<()> {
                Ok(())
            }
        }
        Self {
            persistent: Box::new(SessionOnly),
            session: Mutex::new(HashMap::new()),
        }
    }
    pub fn get(&self, name: &str) -> Result<Option<String>> {
        if let Some(value) = self.get_session(name) {
            return Ok(Some(value));
        }
        self.persistent.get(name)
    }

    /// Reads only the in-memory session store without touching the OS keyring.
    pub fn get_session(&self, name: &str) -> Option<String> {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .map(|entry| entry.value.clone())
    }

    /// Returns whether the credential is durable. A failed keyring write never creates a plaintext file.
    pub fn set(&self, name: &str, secret: &str, persistent: bool) -> Result<bool> {
        if secret.is_empty() {
            return Err(AppError::new(ErrorCode::InvalidInput, "密钥不能为空"));
        }
        let saved = if persistent {
            self.persistent.set(name, secret).is_ok()
        } else {
            // Switching an existing key to session-only must not leave a stale
            // copy in the OS keyring that would resurrect on next launch.
            let _ = self.persistent.delete(name);
            false
        };
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                name.into(),
                SessionSecret {
                    value: secret.into(),
                    durable: saved,
                },
            );
        Ok(saved)
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        let removed = self
            .session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
        let session_only = removed.as_ref().is_some_and(|entry| !entry.durable);
        match self.persistent.delete(name) {
            Ok(()) => Ok(()),
            // Only ignore persistent deletion errors when the secret was never
            // durably stored in the keyring (e.g. keyring locked/unavailable).
            Err(_) if session_only => Ok(()),
            Err(error) => Err(error),
        }
    }
}

pub fn decode_legacy(value: &str) -> Result<String> {
    if !value.starts_with("dpapi:v1:") {
        return Ok(value.into());
    }
    #[cfg(windows)]
    {
        use base64::Engine;
        use windows::Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::Cryptography::{
                CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
            },
        };
        let mut bytes = base64::engine::general_purpose::STANDARD
            .decode(&value[9..])
            .map_err(|_| credentials_error())?;
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_mut_ptr(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
            .map_err(|_| credentials_error())?;
            let decoded = if output.pbData.is_null() || output.cbData == 0 {
                Ok(String::new())
            } else {
                let len = output.cbData as usize;
                let plain = std::slice::from_raw_parts(output.pbData, len).to_vec();
                std::ptr::write_bytes(output.pbData, 0, len);
                String::from_utf8(plain)
            };
            if !output.pbData.is_null() {
                let _ = LocalFree(HLOCAL(output.pbData as *mut _));
            }
            decoded.map_err(|_| credentials_error())
        }
    }
    #[cfg(not(windows))]
    Err(AppError::new(
        ErrorCode::Credentials,
        "旧 DPAPI 密钥只能在原 Windows 账户中导入，请重新填写",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingStore;
    impl SecretStore for FailingStore {
        fn get(&self, _name: &str) -> Result<Option<String>> {
            Err(credentials_error())
        }
        fn set(&self, _name: &str, _secret: &str) -> Result<()> {
            Err(credentials_error())
        }
        fn delete(&self, _name: &str) -> Result<()> {
            Err(credentials_error())
        }
    }

    #[test]
    fn session_secret_can_be_deleted_when_keyring_is_unavailable() {
        let vault = CredentialVault {
            persistent: Box::new(FailingStore),
            session: Mutex::new(HashMap::new()),
        };
        assert!(!vault.set("openai", "sk-test", true).unwrap());
        assert_eq!(vault.get("openai").unwrap().as_deref(), Some("sk-test"));
        assert_eq!(vault.get_session("openai").as_deref(), Some("sk-test"));
        assert_eq!(vault.get_session("deepl"), None);
        vault.delete("openai").unwrap();
        assert!(vault.get("openai").is_err());
    }

    #[test]
    fn durable_secret_delete_surfaces_keyring_failure_and_session_only_clears_persistent() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        struct ControlledStore {
            data: Mutex<HashMap<String, String>>,
            fail_delete: AtomicBool,
        }
        struct SharedStore(Arc<ControlledStore>);
        impl SecretStore for SharedStore {
            fn get(&self, name: &str) -> Result<Option<String>> {
                Ok(self.0.data.lock().unwrap().get(name).cloned())
            }
            fn set(&self, name: &str, secret: &str) -> Result<()> {
                self.0
                    .data
                    .lock()
                    .unwrap()
                    .insert(name.into(), secret.into());
                Ok(())
            }
            fn delete(&self, name: &str) -> Result<()> {
                if self.0.fail_delete.load(Ordering::Relaxed) {
                    return Err(credentials_error());
                }
                self.0.data.lock().unwrap().remove(name);
                Ok(())
            }
        }

        let store = Arc::new(ControlledStore {
            data: Mutex::new(HashMap::new()),
            fail_delete: AtomicBool::new(false),
        });
        let vault = CredentialVault {
            persistent: Box::new(SharedStore(store.clone())),
            session: Mutex::new(HashMap::new()),
        };

        assert!(vault.set("openai", "sk-durable", true).unwrap());
        store.fail_delete.store(true, Ordering::Relaxed);
        assert!(vault.delete("openai").is_err());
        store.fail_delete.store(false, Ordering::Relaxed);

        // Switching to session-only must also clear the old durable entry.
        assert!(!vault.set("openai", "sk-session", false).unwrap());
        assert_eq!(store.data.lock().unwrap().get("openai"), None);
        assert_eq!(vault.get("openai").unwrap().as_deref(), Some("sk-session"));
    }
}
