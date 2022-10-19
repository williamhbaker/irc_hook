use futures::prelude::*;
use irc::client::prelude::*;
use std::env;

#[tokio::main]
async fn main() -> Result<(), irc::error::Error> {
    let resolved_config = ResolvedConfig::build().unwrap();

    let irc_config = Config {
        nickname: Some(resolved_config.nickname),
        nick_password: Some(resolved_config.nick_password),
        server: Some(resolved_config.server),
        use_tls: Some(true),
        ..Config::default()
    };

    let mut client = Client::from_config(irc_config).await?;
    client.identify()?;

    let mut stream = client.stream()?;

    while let Some(message) = stream.next().await.transpose()? {
        handle_msg(message);
    }

    Ok(())
}

pub struct ResolvedConfig {
    pub nickname: String,
    pub nick_password: String,
    pub server: String,
}

impl ResolvedConfig {
    pub fn build() -> Result<ResolvedConfig, &'static str> {
        let nickname = env::var("IRC_HOOK_NICK").unwrap();
        let nick_password = env::var("IRC_HOOK_NICK_PASSWORD").unwrap();
        let server = env::var("IRC_HOOK_SERVER").unwrap();

        Ok(ResolvedConfig {
            nickname,
            nick_password,
            server,
        })
    }
}

fn handle_msg(msg: Message) {
    if let Some(content) = get_content(&msg.to_string()) {
        print!("{}", content)
    }
}

fn get_content(m: &str) -> Option<String> {
    // Skip the first character, which is always a :, then find the next :
    let mut chrs = m.chars().skip(1);

    if let Some(_) = chrs.find(|&c| c == ':') {
        Some(chrs.collect())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_content() {
        let input = ":first part:Hello this is a message".to_string();
        assert_eq!(
            get_content(&input),
            Some("Hello this is a message".to_string())
        );
    }
}
