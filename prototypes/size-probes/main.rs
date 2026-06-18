//! Each feature links one candidate engine and does the minimal work to force linkage, so a
//! release+stripped build reveals its binary footprint. The usage below follows each crate's current
//! API and should be updated with that API when dependencies move.

fn main() {
    #[cfg(feature = "baseline")]
    {
        println!("baseline: empty probe");
    }

    #[cfg(feature = "tokio_runtime")]
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime");
        let value = runtime.block_on(async { 1usize });
        println!("tokio_runtime: value={value}");
    }

    // --- IPFS and Tor candidate profiles ---

    #[cfg(feature = "ipfs_rust_node")]
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let node = runtime.block_on(async {
            rust_ipfs::builder::DefaultIpfsBuilder::new()
                .with_default()
                .start()
                .await
        });
        println!("ipfs_rust_node: started={}", node.is_ok());
    }

    #[cfg(feature = "tor_arti_onion")]
    {
        let client = arti_client::TorClient::builder().create_unbootstrapped();
        println!("tor_arti_onion: initialized={}", client.is_ok());
    }

    #[cfg(feature = "loom_tls_baseline")]
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime");
        let provider = rustls_loom_aws::crypto::aws_lc_rs::default_provider();
        let connector = std::any::type_name::<tokio_rustls::TlsConnector>();
        let value = runtime.block_on(async { provider.cipher_suites.len() });
        println!("loom_tls_baseline: suites={value} connector={connector}");
    }

    // --- L1 guard candidates ---

    #[cfg(feature = "cel")]
    {
        // CEL: a non-Turing-complete expression language. API ~ cel-interpreter 0.10.
        use cel_interpreter::{Context, Program};
        let program = Program::compile("1 == 1").expect("compile");
        let context = Context::default();
        let result = program.execute(&context);
        println!("cel: {result:?}");
    }

    #[cfg(feature = "regorus")]
    {
        // Rego (OPA policy language). API ~ regorus 0.2.x.
        let mut engine = regorus::Engine::new();
        engine
            .add_policy("p.rego".to_string(), "package p\nallow = true".to_string())
            .expect("add policy");
        let results = engine.eval_query("data.p.allow".to_string(), false);
        println!("regorus: {}", results.is_ok());
    }

    // --- L2 derivation candidates ---

    #[cfg(feature = "ascent")]
    {
        // Datalog via a compile-time macro: generates plain Rust, so it should add ~nothing.
        ascent::ascent! {
            relation number(i32);
            relation positive(i32);
            positive(x) <-- number(x), if *x > 0;
        }
        let mut prog = AscentProgram::default();
        prog.number = vec![(1,), (-2,), (3,)];
        prog.run();
        println!("ascent positives: {}", prog.positive.len());
    }

    #[cfg(feature = "cozo")]
    {
        // Cozo: a full transactional Datalog DB (heavyweight). API ~ cozo 0.7.
        let db = cozo::DbInstance::new("mem", "", Default::default()).expect("db");
        let res = db.run_script(
            "?[x] := x = 1",
            Default::default(),
            cozo::ScriptMutability::Immutable,
        );
        println!("cozo: {}", res.is_ok());
    }

    // --- Cron parser candidates (events/triggers exploration) ---
    // The real workload is: parse a schedule once, then compute the next fire instants from a FIXED
    // base time. The base is fixed (not `now()`) on purpose: the upcoming set is then a deterministic
    // function of (expression, base), which is exactly how a Loom trigger pins its fire time as a
    // seeded input. Each block forces linkage and exercises that path.

    #[cfg(feature = "cron")]
    {
        // zslayton/cron: 6- or 7-field expressions (sec min hour dom mon dow [year]). API ~ cron 0.12+.
        use chrono::{TimeZone, Utc};
        use cron::Schedule;
        use std::str::FromStr;
        let schedule = Schedule::from_str("0 0 9 * * Mon-Fri").expect("parse");
        let base = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let next: Vec<_> = schedule.after(&base).take(5).collect();
        println!("cron: next={}", next.len());
    }

    #[cfg(feature = "croner")]
    {
        // Hexagon/croner: POSIX/Vixie-flavored 5- or 6-field expressions. API ~ croner 3.x
        // (Cron implements FromStr; the old `Cron::new(..).parse()` builder was removed in 3.0).
        use chrono::{TimeZone, Utc};
        use croner::Cron;
        use std::str::FromStr;
        let cron = Cron::from_str("0 9 * * 1-5").expect("parse");
        let base = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let next = cron.find_next_occurrence(&base, false).expect("next");
        println!("croner: next={next}");
    }

    #[cfg(feature = "saffron")]
    {
        // cloudflare/saffron: the parser behind Cloudflare Workers Cron Triggers. API ~ saffron 0.1.x.
        use chrono::{TimeZone, Utc};
        use saffron::Cron;
        use std::str::FromStr;
        let cron = Cron::from_str("0 9 * * 1-5").expect("parse");
        let base = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let next = cron.next_after(base);
        println!("saffron: any={} next={next:?}", cron.any());
    }

    // --- Calendar (0037) parse / recurrence candidates ---
    // The real workload is: parse an iCalendar object (the canonical resource) to drive the derived
    // index, and expand an RRULE within a query window. Each block forces linkage of that path.

    #[cfg(feature = "ical")]
    {
        // icalendar: parse/build RFC 5545 VCALENDAR objects. API ~ icalendar 0.16.
        use icalendar::parser;
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a@x\r\nSUMMARY:Hi\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let unfolded = parser::unfold(ics);
        let parsed = parser::read_calendar(&unfolded).expect("parse");
        println!("ical: components={}", parsed.components.len());
    }

    #[cfg(feature = "rrule")]
    {
        // rrule: RFC 5545 RRULE expansion. API ~ rrule 0.12 (RRuleSet implements FromStr).
        use rrule::RRuleSet;
        use std::str::FromStr;
        let set: RRuleSet = "DTSTART:20240101T090000Z\nRRULE:FREQ=WEEKLY;BYDAY=MO;COUNT=5"
            .parse()
            .expect("parse");
        let result = set.all(16);
        println!("rrule: occurrences={}", result.dates.len());
    }

    #[cfg(feature = "ical_rrule")]
    {
        // Both linked together - the calendar facet's actual dependency footprint.
        use icalendar::parser;
        use rrule::RRuleSet;
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a@x\r\nDTSTART:20240101T090000Z\r\nRRULE:FREQ=WEEKLY;BYDAY=MO;COUNT=5\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let unfolded = parser::unfold(ics);
        let parsed = parser::read_calendar(&unfolded).expect("parse");
        let set: RRuleSet = "DTSTART:20240101T090000Z\nRRULE:FREQ=WEEKLY;BYDAY=MO;COUNT=5"
            .parse()
            .expect("parse");
        println!(
            "ical_rrule: components={} occurrences={}",
            parsed.components.len(),
            set.all(16).dates.len()
        );
    }

    #[cfg(feature = "civil_time")]
    {
        // The `time` crate doing proleptic-Gregorian wall-clock math - the substrate a hand-built RRULE
        // engine would expand over (offsets resolved later from each resource's own VTIMEZONE, so no
        // global tz database is linked). API ~ time 0.3.
        use time::{Date, Duration, Month, PrimitiveDateTime, Time};
        let start = PrimitiveDateTime::new(
            Date::from_calendar_date(2024, Month::January, 1).unwrap(),
            Time::from_hms(9, 0, 0).unwrap(),
        );
        // Five weekly occurrences (the same workload as the rrule probe), pure civil math.
        let occ: Vec<_> = (0..5).map(|i| start + Duration::weeks(i)).collect();
        println!(
            "civil_time: occurrences={} first_weekday={:?}",
            occ.len(),
            start.weekday()
        );
    }

    // --- Loom Templates parser/runtime candidates ---
    // Each block renders variable substitution, a loop, and whitespace control. That is enough to
    // force the parser and render path without turning this probe into a benchmark.

    #[cfg(feature = "minijinja_default")]
    {
        use minijinja::{context, Environment};
        let mut env = Environment::new();
        env.add_template(
            "page",
            "Hello {{ name }}:{% for item in items %} {{- item }}{% endfor %}",
        )
        .expect("template");
        let template = env.get_template("page").expect("get template");
        let output = template
            .render(context!(name => "Loom", items => ["a", "b", "c"]))
            .expect("render");
        println!("minijinja_default: {output}");
    }

    #[cfg(feature = "minijinja_minimal")]
    {
        use minijinja_minimal::{context, Environment};
        let mut env = Environment::new();
        env.add_template(
            "page",
            "Hello {{ name }}:{% for item in items %} {{- item }}{% endfor %}",
        )
        .expect("template");
        let template = env.get_template("page").expect("get template");
        let output = template
            .render(context!(name => "Loom", items => ["a", "b", "c"]))
            .expect("render");
        println!("minijinja_minimal: {output}");
    }

    #[cfg(feature = "tera_template")]
    {
        use tera::{Context, Tera};
        let mut engine = Tera::default();
        engine
            .add_raw_template(
                "page",
                "Hello {{ name }}:{% for item in items %} {{- item }}{% endfor %}",
            )
            .expect("template");
        let mut context = Context::new();
        context.insert("name", "Loom");
        context.insert("items", &["a", "b", "c"]);
        let output = engine.render("page", &context).expect("render");
        println!("tera_template: {output}");
    }

    #[cfg(feature = "upon_template")]
    {
        let mut engine = upon::Engine::new();
        engine
            .add_template(
                "page",
                "Hello {{ name }}:{% for item in items %} {{- item }}{% endfor %}",
            )
            .expect("template");
        let output = engine
            .template("page")
            .render(upon::value! { name: "Loom", items: ["a", "b", "c"] })
            .to_string()
            .expect("render");
        println!("upon_template: {output}");
    }

    #[cfg(feature = "handlebars_template")]
    {
        use handlebars::Handlebars;
        use serde_json::json;
        let mut engine = Handlebars::new();
        engine
            .register_template_string("page", "Hello {{name}}:{{#each items}} {{this}}{{/each}}")
            .expect("template");
        let output = engine
            .render("page", &json!({ "name": "Loom", "items": ["a", "b", "c"] }))
            .expect("render");
        println!("handlebars_template: {output}");
    }

    #[cfg(feature = "rustls_default")]
    {
        let roots = rustls::RootCertStore::empty();
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let alpn = std::hint::black_box(config.alpn_protocols.len());
        println!("rustls_default: alpn={alpn}");
    }

    #[cfg(feature = "rustls_ring")]
    {
        let provider = std::sync::Arc::new(rustls_ring::crypto::ring::default_provider());
        let roots = rustls_ring::RootCertStore::empty();
        let config = rustls_ring::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls_ring::version::TLS13])
            .expect("versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        let alpn = std::hint::black_box(config.alpn_protocols.len());
        println!("rustls_ring: alpn={alpn}");
    }

    #[cfg(feature = "rustls_aws")]
    {
        let provider = std::sync::Arc::new(rustls_aws::crypto::aws_lc_rs::default_provider());
        let roots = rustls_aws::RootCertStore::empty();
        let config = rustls_aws::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls_aws::version::TLS13])
            .expect("versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        let alpn = std::hint::black_box(config.alpn_protocols.len());
        println!("rustls_aws: alpn={alpn}");
    }

    #[cfg(feature = "rustls_aws_fips")]
    {
        let provider = std::sync::Arc::new(rustls_aws_fips::crypto::aws_lc_rs::default_provider());
        let roots = rustls_aws_fips::RootCertStore::empty();
        let config = rustls_aws_fips::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls_aws_fips::version::TLS13])
            .expect("versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        let alpn = std::hint::black_box(config.alpn_protocols.len());
        println!("rustls_aws_fips: alpn={alpn}");
    }

    #[cfg(feature = "rustls_pki_types")]
    {
        use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        let cert = CertificateDer::from(vec![0x30, 0x00]);
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(vec![0x30, 0x00]));
        let total = std::hint::black_box(cert.as_ref().len() + key.secret_der().len());
        println!("rustls_pki_types: bytes={total}");
    }

    #[cfg(feature = "rustls_webpki")]
    {
        use rustls_pki_types::{CertificateDer, ServerName};
        let cert = CertificateDer::from(vec![0x30, 0x00]);
        let parsed = webpki::EndEntityCert::try_from(&cert);
        let name = ServerName::try_from("localhost").expect("server name");
        println!(
            "rustls_webpki: parsed={} name={:?}",
            parsed.is_ok(),
            std::hint::black_box(name)
        );
    }

    #[cfg(feature = "x509_parser")]
    {
        use x509_parser::prelude::{FromDer, X509Certificate};
        let parsed = X509Certificate::from_der(std::hint::black_box(&[0x30, 0x00]));
        println!("x509_parser: parsed={}", parsed.is_ok());
    }

    #[cfg(feature = "x509_cert")]
    {
        use x509_cert::der::Decode;
        use x509_cert::Certificate;
        let parsed = Certificate::from_der(std::hint::black_box(&[0x30, 0x00]));
        println!("x509_cert: parsed={}", parsed.is_ok());
    }

    #[cfg(feature = "rcgen_self_signed")]
    {
        let subject_alt_names = vec!["localhost".to_string(), "api.local".to_string()];
        let generated = rcgen::generate_simple_self_signed(std::hint::black_box(subject_alt_names))
            .expect("self signed");
        let total = std::hint::black_box(
            generated.cert.pem().len() + generated.signing_key.serialize_pem().len(),
        );
        println!("rcgen_self_signed: bytes={total}");
    }

    #[cfg(feature = "rcgen_ed25519")]
    {
        let subject_alt_names = vec!["localhost".to_string()];
        let params =
            rcgen::CertificateParams::new(std::hint::black_box(subject_alt_names)).expect("params");
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519).expect("ed25519 key");
        let cert = params.self_signed(&key).expect("ed25519 cert");
        let total = std::hint::black_box(cert.pem().len() + key.serialize_pem().len());
        println!("rcgen_ed25519: bytes={total}");
    }

    #[cfg(feature = "grpc_tonic")]
    {
        use prost::Message;

        #[derive(Clone, PartialEq, ::prost::Message)]
        struct ProbeMessage {
            #[prost(string, tag = "1")]
            value: String,
        }

        let message = ProbeMessage {
            value: "probe".to_string(),
        };
        let encoded = std::hint::black_box(message.encode_to_vec());
        let request = tonic::Request::new(message);
        let response = tonic::Response::new(request.into_inner());
        let status = tonic::Status::new(tonic::Code::Ok, "ok");
        let endpoint = tonic::transport::Endpoint::from_static("http://127.0.0.1:1")
            .user_agent("loom-size-probe")
            .expect("user agent")
            .connect_timeout(std::time::Duration::from_millis(10))
            .http2_keep_alive_interval(std::time::Duration::from_secs(30));
        let channel = std::hint::black_box(endpoint.connect_lazy());
        println!(
            "grpc_tonic: value={} bytes={} status={:?} channel={:?}",
            response.get_ref().value,
            encoded.len(),
            status.code(),
            channel
        );
    }

    #[cfg(feature = "oidc_openidconnect_default")]
    {
        use openidconnect::core::{CoreClient, CoreProviderMetadata};
        use openidconnect::{
            AuthUrl, ClientId, ClientSecret, IssuerUrl, JsonWebKeySetUrl, RedirectUrl,
            ResponseTypes, Scope,
        };
        let metadata = CoreProviderMetadata::new(
            IssuerUrl::new("https://issuer.example".to_string()).expect("issuer"),
            AuthUrl::new("https://issuer.example/auth".to_string()).expect("auth"),
            JsonWebKeySetUrl::new("https://issuer.example/jwks".to_string()).expect("jwks"),
            vec![ResponseTypes::new(vec![
                openidconnect::core::CoreResponseType::Code,
            ])],
            vec![openidconnect::core::CoreSubjectIdentifierType::Public],
            vec![openidconnect::core::CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256],
            openidconnect::EmptyAdditionalProviderMetadata {},
        );
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new("client".to_string()),
            Some(ClientSecret::new("secret".to_string())),
        )
        .set_redirect_uri(
            RedirectUrl::new("https://app.example/callback".to_string()).expect("redirect"),
        );
        let (url, _csrf, _nonce) = client
            .authorize_url(
                openidconnect::core::CoreAuthenticationFlow::AuthorizationCode,
                openidconnect::CsrfToken::new_random,
                openidconnect::Nonce::new_random,
            )
            .add_scope(Scope::new("openid".to_string()))
            .url();
        println!("oidc_openidconnect_default: {}", std::hint::black_box(url));
    }

    #[cfg(feature = "oidc_openidconnect_core")]
    {
        use openidconnect_core::core::{CoreClient, CoreProviderMetadata};
        use openidconnect_core::{
            AuthUrl, ClientId, ClientSecret, IssuerUrl, JsonWebKeySetUrl, RedirectUrl,
            ResponseTypes, Scope,
        };
        let metadata = CoreProviderMetadata::new(
            IssuerUrl::new("https://issuer.example".to_string()).expect("issuer"),
            AuthUrl::new("https://issuer.example/auth".to_string()).expect("auth"),
            JsonWebKeySetUrl::new("https://issuer.example/jwks".to_string()).expect("jwks"),
            vec![ResponseTypes::new(vec![
                openidconnect_core::core::CoreResponseType::Code,
            ])],
            vec![openidconnect_core::core::CoreSubjectIdentifierType::Public],
            vec![openidconnect_core::core::CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256],
            openidconnect_core::EmptyAdditionalProviderMetadata {},
        );
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new("client".to_string()),
            Some(ClientSecret::new("secret".to_string())),
        )
        .set_redirect_uri(
            RedirectUrl::new("https://app.example/callback".to_string()).expect("redirect"),
        );
        let (url, _csrf, _nonce) = client
            .authorize_url(
                openidconnect_core::core::CoreAuthenticationFlow::AuthorizationCode,
                openidconnect_core::CsrfToken::new_random,
                openidconnect_core::Nonce::new_random,
            )
            .add_scope(Scope::new("openid".to_string()))
            .url();
        println!("oidc_openidconnect_core: {}", std::hint::black_box(url));
    }

    // --- Hugging Face / direct HTTP model download candidates (0062) ---

    #[cfg(feature = "hf_hub_default")]
    {
        let api = hf_hub_default::api::sync::ApiBuilder::new()
            .with_progress(false)
            .build()
            .expect("api");
        let repo = api.model("sentence-transformers/all-MiniLM-L6-v2".to_string());
        let path = repo.get("config.json");
        println!("hf_hub_default: cached={}", path.is_ok());
    }

    #[cfg(feature = "hf_hub_tokio_rustls")]
    {
        let api = hf_hub_tokio_rustls::api::tokio::ApiBuilder::new()
            .with_progress(false)
            .build()
            .expect("api");
        let repo = api.model("sentence-transformers/all-MiniLM-L6-v2".to_string());
        let future = repo.get("config.json");
        println!(
            "hf_hub_tokio_rustls: future={}",
            std::any::type_name_of_val(&future)
        );
    }

    #[cfg(feature = "hf_hub_ureq")]
    {
        let api = hf_hub_ureq::api::sync::ApiBuilder::new()
            .with_progress(false)
            .build()
            .expect("api");
        let repo = api.model("sentence-transformers/all-MiniLM-L6-v2".to_string());
        let path = repo.get("config.json");
        println!("hf_hub_ureq: cached={}", path.is_ok());
    }

    #[cfg(feature = "reqwest_rustls_blocking")]
    {
        let client = reqwest_rustls_blocking::blocking::Client::builder()
            .user_agent("loom-size-probe")
            .build()
            .expect("client");
        let request = client
            .get("https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/config.json")
            .build()
            .expect("request");
        println!("reqwest_rustls_blocking: {}", request.url());
    }

    // --- Inference runtime/client candidates (0062 runtime adapter decisions) ---

    #[cfg(feature = "inference_http_baseline")]
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime");
        let provider = rustls_loom_aws::crypto::aws_lc_rs::default_provider();
        let client = reqwest_rustls::Client::builder()
            .user_agent("loom-size-probe")
            .build()
            .expect("client");
        let request = client
            .get("https://localhost:11434/api/version")
            .build()
            .expect("request");
        let suites = runtime.block_on(async { provider.cipher_suites.len() });
        println!(
            "inference_http_baseline: suites={suites} method={}",
            request.method()
        );
    }

    #[cfg(feature = "genai_rustls")]
    {
        let client = genai_probe::Client::default();
        println!("genai_rustls: {}", std::any::type_name_of_val(&client));
    }

    #[cfg(feature = "ollama_rs_rustls")]
    {
        let client = ollama_rs_rustls::Ollama::builder()
            .host("http://localhost")
            .port(11434)
            .build();
        println!("ollama_rs_rustls: {}", std::any::type_name_of_val(&client));
    }

    #[cfg(feature = "ollama_rs_stream")]
    {
        let client = ollama_rs_stream::Ollama::builder()
            .host("http://localhost")
            .port(11434)
            .build();
        let message = ollama_rs_stream::generation::chat::ChatMessage::user("hello".to_string());
        println!(
            "ollama_rs_stream: {} {}",
            std::any::type_name_of_val(&client),
            std::any::type_name_of_val(&message)
        );
    }

    #[cfg(feature = "llama_cpp_2_common")]
    {
        let backend = llama_cpp_2_common::llama_backend::LlamaBackend::init();
        println!("llama_cpp_2_common: backend={}", backend.is_ok());
    }

    #[cfg(feature = "llama_cpp_2_metal")]
    {
        let backend = llama_cpp_2_metal::llama_backend::LlamaBackend::init();
        println!("llama_cpp_2_metal: backend={}", backend.is_ok());
    }

    #[cfg(feature = "mistralrs_default")]
    {
        let text_model = mistralrs_probe::ModelBuilder::new("Qwen/Qwen3-4B")
            .with_auto_isq(mistralrs_probe::IsqBits::Four)
            .with_logging();
        let embedding_model =
            mistralrs_probe::EmbeddingModelBuilder::new("sentence-transformers/all-MiniLM-L6-v2")
                .with_force_cpu()
                .with_max_num_seqs(4);
        let gguf_model = mistralrs_probe::GgufModelBuilder::new(
            "unsloth/Qwen3-4B-GGUF",
            vec!["Qwen3-4B-Q4_K_M.gguf"],
        )
        .with_force_cpu();
        let request = mistralrs_probe::RequestBuilder::new()
            .add_message(mistralrs_probe::TextMessageRole::User, "hello")
            .set_sampler_temperature(0.2)
            .set_sampler_topk(40)
            .set_sampler_topp(0.95)
            .set_sampler_max_len(64);
        let embeddings = mistralrs_probe::EmbeddingRequest::builder().add_prompt("hello");
        println!(
            "mistralrs_default: {} {} {} {} {}",
            std::any::type_name_of_val(&text_model),
            std::any::type_name_of_val(&embedding_model),
            std::any::type_name_of_val(&gguf_model),
            std::any::type_name_of_val(&request),
            std::any::type_name_of_val(&embeddings)
        );
    }

    #[cfg(feature = "apple_mlx")]
    {
        println!(
            "apple_mlx: {}",
            std::any::type_name::<apple_mlx_probe::Complex32>()
        );
    }

    #[cfg(feature = "llmfit_core")]
    {
        println!(
            "llmfit_core: {}",
            std::any::type_name::<llmfit_core_probe::SystemSpecs>()
        );
    }

    // --- Hardware/system probing candidates (0062 doctor) ---

    #[cfg(feature = "hardware_sysinfo")]
    {
        let mut system = sysinfo::System::new_all();
        system.refresh_memory();
        system.refresh_cpu_all();
        println!(
            "hardware_sysinfo: cpus={} total_memory={}",
            system.cpus().len(),
            system.total_memory()
        );
    }

    #[cfg(feature = "hardware_systemstat")]
    {
        use systemstat::{Platform, System};
        let system = System::new();
        let memory = system.memory();
        println!("hardware_systemstat: memory_ok={}", memory.is_ok());
    }

    #[cfg(feature = "hardware_sys_info")]
    {
        let cpu_num = sys_info_probe::cpu_num().unwrap_or_default();
        let memory = sys_info_probe::mem_info().ok();
        println!(
            "hardware_sys_info: cpus={} memory_ok={}",
            cpu_num,
            memory.is_some()
        );
    }

    #[cfg(feature = "oidc_id_token_verifier")]
    {
        let type_name = std::any::type_name::<id_token_verifier::IdTokenVerifierDefault>();
        let iss = id_token_verifier::validation::Iss::new("https://issuer.example");
        let aud = id_token_verifier::validation::Aud::new("loom");
        println!(
            "oidc_id_token_verifier: type={} iss={:?} aud={:?}",
            std::hint::black_box(type_name),
            std::hint::black_box(iss),
            std::hint::black_box(aud)
        );
    }

    #[cfg(feature = "oidc_jsonwebtoken_default")]
    {
        use jsonwebtoken::{Algorithm, DecodingKey, Validation};
        let key = DecodingKey::from_secret(std::hint::black_box(b"secret"));
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&["loom"]);
        println!(
            "oidc_jsonwebtoken_default: required={} key={:?}",
            validation.required_spec_claims.len(),
            std::hint::black_box(key)
        );
    }

    #[cfg(feature = "oidc_jsonwebtoken_aws")]
    {
        use jsonwebtoken_aws::{Algorithm, DecodingKey, Validation};
        let key = DecodingKey::from_secret(std::hint::black_box(b"secret"));
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&["loom"]);
        println!(
            "oidc_jsonwebtoken_aws: required={} key={:?}",
            validation.required_spec_claims.len(),
            std::hint::black_box(key)
        );
    }

    #[cfg(feature = "saml_alpha")]
    {
        let name_id = saml::nameid::NameId::email("user@example.com");
        let cache = saml::replay::InMemoryReplayCache::new(8);
        println!(
            "saml_alpha: name_id={:?} cache_empty={}",
            std::hint::black_box(name_id),
            cache.is_empty()
        );
    }

    #[cfg(feature = "saml_rustauth")]
    {
        let version = rustauth_saml::VERSION;
        let policy = rustauth_saml::SamlRuntimeAlgorithmPolicy::default();
        println!(
            "saml_rustauth: version={} policy={:?}",
            std::hint::black_box(version),
            std::hint::black_box(policy)
        );
    }

    #[cfg(feature = "saml_rustauth_signed")]
    {
        let version = rustauth_saml_signed::VERSION;
        let signed = rustauth_saml_signed::signature::SamlSignatureInfo {
            count: 1,
            response: true,
            assertion: false,
            logout_request: false,
            logout_response: false,
        };
        println!(
            "saml_rustauth_signed: version={} signed={}",
            std::hint::black_box(version),
            signed.is_signed()
        );
    }

    #[cfg(feature = "saml_opensaml")]
    {
        let parsed = opensaml::context::is_valid_xml(
            r#"<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol"/>"#,
        );
        println!("saml_opensaml: parsed={}", parsed.is_ok());
    }

    #[cfg(feature = "saml_opensaml_protocol")]
    {
        let parsed = opensaml_protocol::context::is_valid_xml(
            r#"<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol"/>"#,
        );
        println!("saml_opensaml_protocol: parsed={}", parsed.is_ok());
    }

    #[cfg(feature = "webauthn_rp")]
    {
        use webauthn_rp::request::register::{
            AuthenticatorSelectionCriteria, CoseAlgorithmIdentifier,
        };
        let selection = AuthenticatorSelectionCriteria::passkey();
        let alg = CoseAlgorithmIdentifier::Es256;
        println!(
            "webauthn_rp: selection={:?} alg={:?}",
            std::hint::black_box(selection),
            std::hint::black_box(alg)
        );
    }

    #[cfg(feature = "webauthn_caden")]
    {
        let relying_party =
            webauthn::RelyingParty::new("example.com", "https://example.com", "Loom");
        let challenge = webauthn::Challenge::new().expect("challenge");
        println!(
            "webauthn_caden: rp={:?} challenge={}",
            std::hint::black_box(relying_party),
            challenge.bytes.len()
        );
    }

    #[cfg(feature = "webauthn_passkey")]
    {
        let challenge: passkey::types::Bytes = passkey::types::rand::random_vec(32).into();
        let hash = passkey::types::crypto::sha256(std::hint::black_box(&challenge));
        println!("webauthn_passkey: hash={}", hash.len());
    }

    #[cfg(feature = "webauthn_rs")]
    {
        let origin = url::Url::parse("https://example.com").expect("url");
        let builder = webauthn_rs::WebauthnBuilder::new("example.com", &origin);
        println!(
            "webauthn_rs: builder={}",
            std::hint::black_box(builder).is_ok()
        );
    }

    #[cfg(feature = "x509_parser_verify_ring")]
    {
        use x509_parser_verify_ring::prelude::{FromDer, X509Certificate};
        let parsed = X509Certificate::from_der(std::hint::black_box(&[0x30, 0x00]));
        println!("x509_parser_verify_ring: parsed={}", parsed.is_ok());
    }

    #[cfg(feature = "x509_parser_verify_aws")]
    {
        use x509_parser_verify_aws::prelude::{FromDer, X509Certificate};
        let parsed = X509Certificate::from_der(std::hint::black_box(&[0x30, 0x00]));
        println!("x509_parser_verify_aws: parsed={}", parsed.is_ok());
    }

    #[cfg(feature = "x509_verify_default")]
    {
        let type_name = std::any::type_name::<x509_verify::VerifyingKey>();
        println!(
            "x509_verify_default: type={}",
            std::hint::black_box(type_name)
        );
    }

    #[cfg(feature = "public_key_ring")]
    {
        let public_key = ring::signature::UnparsedPublicKey::new(
            &ring::signature::ED25519,
            std::hint::black_box([0u8; 32]),
        );
        let result = public_key.verify(
            std::hint::black_box(b"message"),
            std::hint::black_box(&[0u8; 64]),
        );
        println!("public_key_ring: verified={}", result.is_ok());
    }

    #[cfg(feature = "public_key_ed25519_dalek")]
    {
        use ed25519_signature::Verifier;
        let verifying_key =
            ed25519_dalek::VerifyingKey::from_bytes(std::hint::black_box(&[0u8; 32]));
        let verified = verifying_key
            .as_ref()
            .map(|key| {
                key.verify(
                    std::hint::black_box(b"message"),
                    &ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                )
            })
            .map(|result| result.is_ok())
            .unwrap_or(false);
        println!("public_key_ed25519_dalek: verified={verified}");
    }

    #[cfg(feature = "public_key_p256")]
    {
        use p256::ecdsa::signature::Verifier;
        let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(std::hint::black_box(&[0x04; 65]));
        let signature = p256::ecdsa::Signature::from_slice(std::hint::black_box(&[0u8; 64]));
        let verified = match (key.as_ref(), signature.as_ref()) {
            (Ok(key), Ok(signature)) => key
                .verify(std::hint::black_box(b"message"), signature)
                .is_ok(),
            _ => false,
        };
        println!("public_key_p256: verified={verified}");
    }

    #[cfg(feature = "public_key_rsa")]
    {
        let key_type = std::any::type_name::<rsa::pkcs1v15::VerifyingKey<rsa::sha2::Sha256>>();
        let signature_type = std::any::type_name::<rsa::pkcs1v15::Signature>();
        println!(
            "public_key_rsa: key={} signature={}",
            std::hint::black_box(key_type),
            std::hint::black_box(signature_type)
        );
    }
}
