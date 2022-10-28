use futures::prelude::*;
use irc::client::prelude::*;
use regex::Regex;
use std::{collections::HashMap, env, str::FromStr, sync::Arc};
use tokio::task;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), irc::error::Error> {
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    tracing::info!("starting irc_hook");

    let resolved_config = ResolvedConfig::build().unwrap();

    let irc_config = Config {
        nickname: Some(resolved_config.nickname),
        nick_password: Some(resolved_config.nick_password),
        server: Some(resolved_config.server),
        use_tls: Some(true),
        ..Config::default()
    };

    let uri = http::Uri::from_str(&env::var("IRC_HOOK_WEBHOOK_URL").unwrap()).unwrap();

    let publisher = WebhookPublisher::new(
        uri,
        env::var("IRC_HOOK_WEBHOOK_API_KEY").unwrap(),
        env::var("IRC_HOOK_BODY_TEMPLATE").unwrap(),
    );

    let mut handler = MessageHandler::new(
        env::var("IRC_HOOK_SEARCH_PATTERN").unwrap(),
        env::var("IRC_HOOK_MULTI_LINE").is_ok(),
        4,
        env::var("IRC_HOOK_LINE_INIT_PATTERN").unwrap(),
        "".to_string(),
        publisher,
    );

    let mut client = Client::from_config(irc_config).await?;
    client.identify()?;

    let mut stream = client.stream()?;

    while let Some(message) = stream.next().await.transpose()? {
        handler.handle_msg(message).await;
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
    message_publisher: WebhookPublisher,

    lines_buffer: Vec<String>,
}

impl MessageHandler {
    fn new(
        search_pattern: String,
        multi_line: bool,
        line_limit: i32,
        line_init_pattern: String,
        line_conclude_pattern: String,
        message_publisher: WebhookPublisher,
    ) -> Self {
        tracing::info!(%line_init_pattern, "starting handler with line init pattern");

        MessageHandler {
            search_pattern,
            multi_line,
            line_limit,
            line_init_pattern,
            line_conclude_pattern,
            message_publisher,
            lines_buffer: vec![],
        }
    }

    async fn handle_msg(&mut self, msg: Message) {
        if let Some(content) = get_content(&msg.to_string()) {
            let re = Regex::new(&self.search_pattern).unwrap();
            let init = Regex::new(&self.line_init_pattern).unwrap();
            let finish = Regex::new(&self.line_conclude_pattern).unwrap();

            if !self.multi_line {
                if !re.is_match(&content) {
                    tracing::debug!(%content, "single-line mode did not match");
                    return;
                }
                tracing::info!(%content, "single-line mode match");

                let groups = match_groups(&re, &content);
                self.message_publisher.publish(groups).await;
            } else {
                if self.lines_buffer.len() == 0 {
                    if init.is_match(&content) {
                        tracing::info!(matched = %content, "starting new multi-line match");
                        self.lines_buffer.push(content)
                    } else {
                        tracing::debug!(content = %content, matcher = %self.line_init_pattern, "multi-line mode did not match for init");
                    }

                    // TODO: Handle line limit of 1

                    return;
                } else {
                    tracing::info!(added = %content, "adding to in-progress multi-line match");

                    self.lines_buffer.push(content.clone());

                    if self.lines_buffer.len() == self.line_limit as usize
                        || self.line_conclude_pattern != "".to_string() && finish.is_match(&content)
                    {
                        tracing::info!(finished_line = %content, "concluded multi-line match");

                        let total = self.lines_buffer.join(" ");
                        tracing::info!(candidate = %total, "will match on combined result");

                        self.lines_buffer = vec![];

                        let groups = match_groups(&re, &total);
                        tracing::info!(?groups, "matched groups");
                        self.message_publisher.publish(groups).await;
                    }
                }
            }
        }
    }
}

fn match_groups(re: &regex::Regex, content: &str) -> Vec<Vec<String>> {
    re.captures_iter(content)
        .map(|group| {
            group
                .iter()
                .filter_map(|mat| Some(mat?.as_str().to_string()))
                .collect()
        })
        .collect()
}

#[derive(Debug)]
struct WebhookPublisher {
    endpoint: http::Uri,
    api_key: String,
    template: String,
}

impl WebhookPublisher {
    fn new(endpoint: http::Uri, api_key: String, template: String) -> Self {
        WebhookPublisher {
            endpoint,
            api_key,
            template,
        }
    }

    async fn publish(&self, params: Vec<Vec<String>>) {
        let ep = Arc::new(self.endpoint.to_string());
        let key = Arc::new(self.api_key.clone());
        let template = Arc::new(self.template.clone());

        let tasks = params
            .into_iter()
            .map(|p_set| {
                task::spawn({
                    let ep_clone = Arc::clone(&ep);
                    let key_clone = Arc::clone(&key);
                    let template_clone = Arc::clone(&template);

                    async move {
                        println!("{:#?}", p_set);

                        // Replace the items in the template with matches from the group.
                        // let mut body_init = template_clone.clone();
                        // p_set.iter().enumerate().fold(body_init, |body, next| {});

                        let mut map = HashMap::new();
                        map.insert("lang", "rust");
                        map.insert("body", "json");

                        let client = reqwest::Client::new();
                        let res = client
                            .post(ep_clone.as_str())
                            .header("X-Api-Key", key_clone.to_string())
                            .json(&map)
                            .send()
                            .await;

                        println!("{:#?}", res);
                    }
                })
            })
            .collect::<stream::FuturesUnordered<_>>();

        let result = futures::future::join_all(tasks).await;
        println!("{:?}", result);
    }
}

fn get_content(m: &str) -> Option<String> {
    // Skip the first character, which is always a :, then find the next :
    let mut chrs = m.chars().skip(1);

    if let Some(_) = chrs.find(|&c| c == ':') {
        // Make sure the trailing newline is removed.
        Some(chrs.collect::<String>().trim().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httptest::{matchers::*, responders::*, Expectation, Server};

    #[test]
    fn test_get_content() {
        let input = ":first part:Hello this is a message".to_string();
        assert_eq!(
            get_content(&input),
            Some("Hello this is a message".to_string())
        );
    }

    #[test]
    fn test_match_groups() {
        let content = r#"Main message 1capture match2 text 1another match2"#;

        let search_pattern = r#"\d(.+?)\d"#;
        let re = Regex::new(&search_pattern).unwrap();

        let got = match_groups(&re, content);

        assert_eq!(
            got,
            vec![
                vec!["1capture match2".to_string(), "capture match".to_string()],
                vec!["1another match2".to_string(), "another match".to_string()]
            ]
        )
    }

    #[tokio::test]
    async fn test_handler_not_multi_line() {
        let content = r#"Main message 1capture match2 text 1another match2"#;
        let search_pattern = r#"\d(.+?)\d"#;

        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("POST", "/endpoint"))
                .times(2)
                .respond_with(status_code(200)),
        );
        let uri = server.url("/endpoint");

        let publisher = WebhookPublisher::new(uri, "".to_string(), "".to_string());
        let mut handler = MessageHandler::new(
            search_pattern.to_string(),
            false,
            0,
            "".to_string(),
            "".to_string(),
            publisher,
        );

        let msg = Message::new(Some("user"), "PRIVMSG", vec!["#channel", content]).unwrap();

        handler.handle_msg(msg).await;
    }

    #[tokio::test]
    async fn test_handler_multi_line() {
        let contents = vec![
            r#"something irrelevant"#,
            r#"**STARTED** more words"#,
            r#"Main message 1capture match2 text"#,
            r#"No matches here"#,
            r#"Another message 1another match match2 more stuff"#,
            r#"something else irrelevant"#,
        ];

        let search_pattern = r#"\d(.+?)\d"#;

        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("POST", "/endpoint"))
                .times(2)
                .respond_with(status_code(200)),
        );
        let uri = server.url("/endpoint");

        let publisher = WebhookPublisher::new(uri, "".to_string(), "".to_string());
        let mut handler = MessageHandler::new(
            search_pattern.to_string(),
            true,
            4,
            r#"\*\*STARTED\*\*"#.to_string(),
            "".to_string(),
            publisher,
        );

        for content in contents.iter() {
            let msg = Message::new(Some("user"), "PRIVMSG", vec!["#channel", content]).unwrap();
            handler.handle_msg(msg).await;
        }
    }

    #[tokio::test]
    async fn test_handler_multi_line_ending() {
        let contents = vec![
            r#"something irrelevant"#,
            r#"**STARTED** more words"#,
            r#"Main message 1capture match2 text"#,
            r#"**FINISHED** more words"#,
            r#"Another message 1another match match2 more stuff"#,
            r#"something else irrelevant"#,
        ];

        let search_pattern = r#"\d(.+?)\d"#;

        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("POST", "/endpoint"))
                .times(1)
                .respond_with(status_code(200)),
        );
        let uri = server.url("/endpoint");

        let publisher = WebhookPublisher::new(uri, "".to_string(), "".to_string());
        let mut handler = MessageHandler::new(
            search_pattern.to_string(),
            true,
            4,
            r#"\*\*STARTED\*\*"#.to_string(),
            r#"\*\*FINISHED\*\*"#.to_string(),
            publisher,
        );

        for content in contents.iter() {
            let msg = Message::new(Some("user"), "PRIVMSG", vec!["#channel", content]).unwrap();
            handler.handle_msg(msg).await;
        }
    }
}
