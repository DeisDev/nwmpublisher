use std::time::Duration;

use ureq::{
	config::Config,
	tls::{TlsConfig, TlsProvider},
	Agent,
};

pub fn agent(timeout: Duration) -> Agent {
	Config::builder()
		.timeout_global(Some(timeout))
		.tls_config(TlsConfig::builder().provider(TlsProvider::NativeTls).build())
		.build()
		.new_agent()
}
