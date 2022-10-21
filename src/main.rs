use futures::prelude::*;
use irc::client::prelude::*;
use regex::Regex;
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

    let handler = MessageHandler::new(
        env::var("IRC_HOOK_SEARCH_PATTERN").unwrap(),
        false,
        0,
        "".to_string(),
        "".to_string(),
    );

    let mut client = Client::from_config(irc_config).await?;
    client.identify()?;

    let mut stream = client.stream()?;

    while let Some(message) = stream.next().await.transpose()? {
        handler.handle_msg(message);
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

struct MessageHandler {
    search_pattern: String,
    multi_line: bool,
    line_limit: i32,
    line_init_pattern: String,
    line_conclude_pattern: String,
}

impl MessageHandler {
    fn new(
        search_pattern: String,
        multi_line: bool,
        line_limit: i32,
        line_init_pattern: String,
        line_conclude_pattern: String,
    ) -> Self {
        MessageHandler {
            search_pattern,
            multi_line,
            line_limit,
            line_init_pattern,
            line_conclude_pattern,
        }
    }

    fn handle_msg(&self, msg: Message) -> Vec<String> {
        let mut out = vec![];

        if let Some(content) = get_content(&msg.to_string()) {
            let re = Regex::new(&self.search_pattern).unwrap();

            if re.is_match(&content) {
                re.captures_iter(&content).for_each(|cap| {
                    cap.iter().for_each(|mat| {
                        if let Some(mat) = mat {
                            out.push(mat.as_str().to_string())
                        }
                    });
                })
            }

            print!("{}", content);
        }

        out
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

    #[test]
    fn test_handle_msg() {
        let content = r#"Main message 1capture match2"#;
        let search_pattern = r#"\d(.+)\d"#.to_string();

        let handler = MessageHandler::new(search_pattern, false, 0, "".to_string(), "".to_string());

        let msg = Message::new(Some("user"), "PRIVMSG", vec!["#channel", content]).unwrap();

        let got = handler.handle_msg(msg);

        assert_eq!(got, vec!["1capture match2", "capture match"])
    }
}
