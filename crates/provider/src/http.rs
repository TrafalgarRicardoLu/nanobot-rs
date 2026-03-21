use crate::ProviderError;
use reqwest::Method;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

pub trait HttpExecutor: Send + Sync {
    fn execute(&self, request: &HttpRequest) -> Result<String, ProviderError>;
}

#[derive(Debug, Clone, Default)]
pub struct ReqwestExecutor;

impl HttpExecutor for ReqwestExecutor {
    fn execute(&self, request: &HttpRequest) -> Result<String, ProviderError> {
        let method = Method::from_bytes(request.method.as_bytes())
            .map_err(|error| ProviderError::Request(error.to_string()))?;

        let mut headers = HeaderMap::new();
        for (name, value) in &request.headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| ProviderError::Request(error.to_string()))?;
            let header_value = HeaderValue::from_str(value)
                .map_err(|error| ProviderError::Request(error.to_string()))?;
            headers.insert(header_name, header_value);
        }

        let client = Client::new();
        let response = client
            .request(method, &request.url)
            .headers(headers)
            .body(request.body.clone())
            .send()
            .map_err(|error| ProviderError::Request(error.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .map_err(|error| ProviderError::Request(error.to_string()))?;
        if !status.is_success() {
            let message = if body.trim().is_empty() {
                status.to_string()
            } else {
                format!("{status}: {}", body.trim())
            };
            return Err(ProviderError::Request(message));
        }

        Ok(body)
    }
}

pub type CurlExecutor = ReqwestExecutor;
