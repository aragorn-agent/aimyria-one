const fs = require('fs');
const p = 'D:\\src\\cc-switch-main\\src-tauri\\src\\services\\stream_check.rs';
let L = fs.readFileSync(p, 'utf8').split(/\r?\n/);

if (L[139].trim() !== 'let response_time = start.elapsed().as_millis() as u64;') {
  console.error('a1: ' + JSON.stringify(L[139])); process.exit(1);
}
if (L[149].trim() !== '))') {
  console.error('a2: ' + JSON.stringify(L[149])); process.exit(1);
}

const repl = [
  "        // A plain GET to the base URL: any HTTP response proves the port is",
  "        // reachable, so there is no need to replay the provider's real call.",
  "        let client = Client::builder()",
  "            .timeout(timeout)",
  "            .build()",
  "            .map_err(|e| AppError::Message(format!(\"failed to build HTTP client: {e}\")))?;",
  "        let mut request = client.get(&base_url);",
  "        if let Some(ua) = ua {",
  "            request = request.header(reqwest::header::USER_AGENT, ua);",
  "        }",
  "",
  "        let response_time = start.elapsed().as_millis() as u64;",
  "        let status = match request.send().await {",
  "            Ok(resp) => resp.status().as_u16(),",
  "            Err(e) => return Err(Self::map_request_error(e)),",
  "        };",
  "        Ok(Self::build_result(",
  "            status,",
  "            response_time,",
  "            config.degraded_threshold_ms,",
  "        ))",
];

L.splice(139, 11, ...repl);
fs.writeFileSync(p, L.join('\n'), 'utf8');
console.log('-> ' + L.length + ' lines');