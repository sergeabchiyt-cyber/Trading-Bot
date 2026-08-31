use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::Value;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

pub struct BinanceClient {
    client: Client,
    api_key: String,
    secret: String,
    base_url: String,
}

impl BinanceClient {
    pub fn new_testnet(api_key: String, secret: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            secret,
            base_url: "https://testnet.binancefuture.com".into(),
        }
    }

    fn timestamp(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    fn sign(&self, query: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes()).unwrap();
        mac.update(query.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    async fn signed_request(
        &self,
        method: reqwest::Method,
        path: &str,
        params: &str,
    ) -> Result<Value> {
        let ts = self.timestamp();
        let query = if params.is_empty() {
            format!("timestamp={}", ts)
        } else {
            format!("{}&timestamp={}", params, ts)
        };
        let signature = self.sign(&query);
        let url = format!("{}{}?{}&signature={}", self.base_url, path, query, signature);

        let resp = self
            .client
            .request(method, &url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?
            .json::<Value>()
            .await?;

        if let Some(code) = resp.get("code") {
            return Err(anyhow!(
                "Binance error {}: {}",
                code,
                resp.get("msg").cloned().unwrap_or_default()
            ));
        }
        Ok(resp)
    }

    pub async fn fetch_klines(
        &self,
        symbol: &str,
        interval: &str,
        limit: u32,
    ) -> Result<Vec<Value>> {
        let url = format!(
            "{}/fapi/v1/klines?symbol={}&interval={}&limit={}",
            self.base_url, symbol, interval, limit
        );
        let resp = self.client.get(&url).send().await?.json().await?;
        Ok(resp)
    }

    pub async fn set_leverage(&self, symbol: &str, leverage: u32) -> Result<Value> {
        let params = format!("symbol={}&leverage={}", symbol, leverage);
        self.signed_request(reqwest::Method::POST, "/fapi/v1/leverage", &params)
            .await
    }

    pub async fn place_order(&self, order: &super::types::OrderRequest) -> Result<Value> {
        let mut params = vec![
            format!("symbol={}", order.symbol),
            format!("side={}", order.side),
            format!("type={}", order.order_type),
            format!("quantity={}", order.quantity),
        ];
        if let Some(p) = order.price {
            params.push(format!("price={}", p));
        }
        if let Some(sp) = order.stop_price {
            params.push(format!("stopPrice={}", sp));
        }
        if let Some(tif) = &order.time_in_force {
            params.push(format!("timeInForce={}", tif));
        }
        if let Some(ro) = order.reduce_only {
            params.push(format!("reduceOnly={}", ro));
        }
        let query = params.join("&");
        self.signed_request(reqwest::Method::POST, "/fapi/v1/order", &query)
            .await
    }

    pub async fn order_status(&self, symbol: &str, order_id: u64) -> Result<Value> {
        let params = format!("symbol={}&orderId={}", symbol, order_id);
        self.signed_request(reqwest::Method::GET, "/fapi/v1/order", &params)
            .await
    }

    pub async fn cancel_order(&self, symbol: &str, order_id: u64) -> Result<Value> {
        let params = format!("symbol={}&orderId={}", symbol, order_id);
        self.signed_request(reqwest::Method::DELETE, "/fapi/v1/order", &params)
            .await
    }

    pub async fn account_balance(&self) -> Result<Value> {
        self.signed_request(reqwest::Method::GET, "/fapi/v2/balance", "")
            .await
    }

    pub async fn position_risk(&self, symbol: &str) -> Result<Value> {
        let params = format!("symbol={}", symbol);
        self.signed_request(reqwest::Method::GET, "/fapi/v2/positionRisk", &params)
            .await
    }

    pub async fn mark_price(&self, symbol: &str) -> Result<f64> {
        let url = format!(
            "{}/fapi/v1/ticker/price?symbol={}",
            self.base_url, symbol
        );
        let v: Value = self.client.get(&url).send().await?.json().await?;
        Ok(v["price"].as_str().unwrap_or("0").parse().unwrap_or(0.0))
    }
}
