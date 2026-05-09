//! Kiro CLI 端点实现

use reqwest::RequestBuilder;
use uuid::Uuid;

use super::{KiroEndpoint, KiroService, RequestContext, UsageRequestParts, service_host};

pub const CLI_ENDPOINT_NAME: &str = "cli";
const CLI_ORIGIN: &str = "KIRO_CLI";
const CLI_API_TARGET: &str = "AmazonQDeveloperStreamingService.SendMessage";
const CLI_STREAMING_API_VERSION: &str = "0.1.14474";
const CLI_RUNTIME_API_VERSION: &str = "0.1.14474";
const CLI_RUST_SDK_VERSION: &str = "1.3.14";
const CLI_RUST_VERSION: &str = "1.92.0";
const CLI_APP_VERSION: &str = "1.29.3";

pub struct CliEndpoint;

impl CliEndpoint {
    pub fn new() -> Self {
        Self
    }

    fn host(&self, ctx: &RequestContext<'_>) -> String {
        service_host(ctx, KiroService::Runtime)
    }

    fn management_host(&self, ctx: &RequestContext<'_>) -> String {
        service_host(ctx, KiroService::Management)
    }

    fn api_origin(&self) -> &'static str {
        CLI_ORIGIN
    }

    fn streaming_user_agent(&self) -> String {
        format!(
            "aws-sdk-rust/{sdk} ua/2.1 api/codewhispererstreaming/{api} os/linux lang/rust/{rust} md/appVersion-{app} app/AmazonQ-For-CLI",
            sdk = CLI_RUST_SDK_VERSION,
            api = CLI_STREAMING_API_VERSION,
            rust = CLI_RUST_VERSION,
            app = CLI_APP_VERSION,
        )
    }

    fn streaming_x_amz_user_agent(&self) -> String {
        format!(
            "aws-sdk-rust/{sdk} ua/2.1 api/codewhispererstreaming/{api} os/linux lang/rust/{rust} m/F app/AmazonQ-For-CLI",
            sdk = CLI_RUST_SDK_VERSION,
            api = CLI_STREAMING_API_VERSION,
            rust = CLI_RUST_VERSION,
        )
    }

    fn runtime_user_agent(&self) -> String {
        format!(
            "aws-sdk-rust/{sdk} ua/2.1 api/codewhispererruntime/{api} os/linux lang/rust/{rust} md/appVersion-{app} app/AmazonQ-For-CLI",
            sdk = CLI_RUST_SDK_VERSION,
            api = CLI_RUNTIME_API_VERSION,
            rust = CLI_RUST_VERSION,
            app = CLI_APP_VERSION,
        )
    }

    fn runtime_x_amz_user_agent(&self) -> String {
        format!(
            "aws-sdk-rust/{sdk} ua/2.1 api/codewhispererruntime/{api} os/linux lang/rust/{rust} m/F app/AmazonQ-For-CLI",
            sdk = CLI_RUST_SDK_VERSION,
            api = CLI_RUNTIME_API_VERSION,
            rust = CLI_RUST_VERSION,
        )
    }

    fn rewrite_origin(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(origin) = map.get_mut("origin") {
                    *origin = serde_json::Value::String(CLI_ORIGIN.to_string());
                }
                for child in map.values_mut() {
                    Self::rewrite_origin(child);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    Self::rewrite_origin(item);
                }
            }
            _ => {}
        }
    }

    fn transform_body(&self, body: &str) -> anyhow::Result<String> {
        let mut request: serde_json::Value = serde_json::from_str(body)?;
        Self::rewrite_origin(&mut request);
        Ok(serde_json::to_string(&request)?)
    }
}

impl Default for CliEndpoint {
    fn default() -> Self {
        Self::new()
    }
}

impl KiroEndpoint for CliEndpoint {
    fn name(&self) -> &'static str {
        CLI_ENDPOINT_NAME
    }

    fn api_url(&self, ctx: &RequestContext<'_>) -> String {
        format!("https://{}/generateAssistantResponse", self.host(ctx))
    }

    fn mcp_url(&self, ctx: &RequestContext<'_>) -> String {
        format!("https://{}/mcp", self.host(ctx))
    }

    fn decorate_api(&self, req: RequestBuilder, ctx: &RequestContext<'_>) -> RequestBuilder {
        let mut req = req
            .header("Accept", "*/*")
            .header("X-Amz-Target", CLI_API_TARGET)
            .header("x-amzn-codewhisperer-optout", "false")
            .header("x-amzn-kiro-agent-mode", "cli")
            .header("x-amz-user-agent", self.streaming_x_amz_user_agent())
            .header("user-agent", self.streaming_user_agent())
            .header("host", self.host(ctx))
            .header("amz-sdk-invocation-id", Uuid::new_v4().to_string())
            .header("amz-sdk-request", "attempt=1; max=3")
            .header("Authorization", format!("Bearer {}", ctx.token));

        if ctx.credentials.is_api_key_credential() {
            req = req.header("tokentype", "API_KEY");
        }
        req
    }

    fn decorate_mcp(&self, req: RequestBuilder, ctx: &RequestContext<'_>) -> RequestBuilder {
        let mut req = req
            .header("x-amz-user-agent", self.streaming_x_amz_user_agent())
            .header("user-agent", self.streaming_user_agent())
            .header("host", self.host(ctx))
            .header("amz-sdk-invocation-id", Uuid::new_v4().to_string())
            .header("amz-sdk-request", "attempt=1; max=3")
            .header("Authorization", format!("Bearer {}", ctx.token));

        if ctx.credentials.is_api_key_credential() {
            req = req.header("tokentype", "API_KEY");
        }
        req
    }

    fn transform_api_body(&self, body: &str, _ctx: &RequestContext<'_>) -> anyhow::Result<String> {
        self.transform_body(body)
    }

    fn transform_mcp_body(&self, body: &str, _ctx: &RequestContext<'_>) -> anyhow::Result<String> {
        self.transform_body(body)
    }

    fn usage_request_parts(&self, ctx: &RequestContext<'_>) -> anyhow::Result<UsageRequestParts> {
        let host = self.management_host(ctx);
        let url = format!(
            "https://{}/getUsageLimits?origin={}&resourceType=AGENTIC_REQUEST",
            host,
            self.api_origin()
        );

        let mut headers = vec![
            ("Accept", "application/json".to_string()),
            ("x-amz-user-agent", self.runtime_x_amz_user_agent()),
            ("user-agent", self.runtime_user_agent()),
            ("host", host),
            ("amz-sdk-invocation-id", Uuid::new_v4().to_string()),
            ("amz-sdk-request", "attempt=1; max=1".to_string()),
            ("Authorization", format!("Bearer {}", ctx.token)),
        ];

        if ctx.credentials.is_api_key_credential() {
            headers.push(("tokentype", "API_KEY".to_string()));
        }

        Ok(UsageRequestParts { url, headers })
    }
}
