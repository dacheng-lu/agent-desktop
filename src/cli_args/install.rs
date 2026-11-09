use clap::Args;

#[derive(Args, Debug)]
pub(crate) struct InstallArgs {
    #[arg(
        value_name = "PROVIDER",
        help = "Provider to install; omit it to report every provider's state"
    )]
    pub provider: Option<String>,
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
