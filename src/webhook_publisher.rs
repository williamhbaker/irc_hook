use http::HeaderMap;
use std::{collections::HashMap, sync::Arc};
use tokio::task;

pub struct WebhookPublisher {
    client: Arc<reqwest::Client>,
    config: Arc<EndpointConfig>,
    template: String,
    headers: HashMap<&'static str, String>,
}

struct EndpointConfig {
    endpoint: http::Uri,
}

impl WebhookPublisher {
    pub fn new(
        endpoint: http::Uri,
        template: String,
        headers: HashMap<&'static str, String>,
    ) -> Self {
        WebhookPublisher {
            client: Arc::new(reqwest::Client::new()),
            config: Arc::new(EndpointConfig { endpoint }),
            template,
            headers,
        }
    }

    pub async fn publish(&self, matched_groups: Vec<Vec<String>>) {
        let tasks = matched_groups
            .iter()
            .map(|g| self.publish_group(g.to_vec()))
            .collect::<Vec<task::JoinHandle<()>>>();

        let result = futures::future::join_all(tasks).await;
        println!("{:?}", result); // TODO: Error propagation.
    }

    pub fn publish_group(&self, group: Vec<String>) -> task::JoinHandle<()> {
        let body = templ_replace(&self.template, &group);
        let headers = to_headers(&self.headers, &group);

        let client = self.client.clone();
        let config = self.config.clone();

        let join = task::spawn({
            async move {
                let res = client
                    .post(config.endpoint.to_string())
                    .body(body)
                    .headers(headers)
                    .send()
                    .await;

                match res {
                    Ok(r) => tracing::info!(post_response = ?r),
                    Err(e) => tracing::error!("webhook POST error: {}", e),
                }
            }
        });

        return join;
    }
}

fn templ_replace(templ: &str, group: &[String]) -> String {
    group
        .iter()
        .enumerate()
        .fold(templ.to_string().clone(), |body, (idx, repl)| {
            let repl_idx = format!("${{{}}}", idx);
            body.replace(&repl_idx, repl)
        })
}

fn to_headers(headers: &HashMap<&'static str, String>, group: &[String]) -> HeaderMap {
    headers
        .iter()
        .fold(http::HeaderMap::new(), |mut accum, (&k, v)| {
            accum.insert(k, templ_replace(v, group).parse().unwrap());
            accum
        })
}
