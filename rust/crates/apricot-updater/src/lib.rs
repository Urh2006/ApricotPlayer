//! App and component update policy. Local Rust betas explicitly disable remote
//! publication and installation.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateChannel {
    Stable,
    Beta,
    LocalOnly,
}

impl UpdateChannel {
    pub const fn allows_remote_install(self) -> bool {
        !matches!(self, Self::LocalOnly)
    }
}

#[cfg(test)]
mod tests {
    use super::UpdateChannel;

    #[test]
    fn local_rust_betas_cannot_install_remote_updates() {
        assert!(!UpdateChannel::LocalOnly.allows_remote_install());
    }
}
