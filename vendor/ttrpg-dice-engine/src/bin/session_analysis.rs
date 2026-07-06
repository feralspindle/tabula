use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufWriter, Write};
use ttrpg_dice_engine::{distribution, RollCategory};

#[derive(Debug, Deserialize)]
struct SessionDieResult {
    die: String,
    value: i64,
}

#[derive(Debug, Deserialize)]
struct RollRecord {
    display_name: String,
    modifier: i64,
    results: Vec<SessionDieResult>,
    total: i64,
    label: Option<String>,
}

/// A rustls certificate verifier that encrypts the connection but does not
/// verify the server's certificate chain. This matches the default behavior of
/// libpq / psql for a Postgres URL with no `sslmode` (i.e. `prefer`/`require`),
/// and is how Supabase's own connection examples behave. Traffic is encrypted
/// in transit but not protected against an active man-in-the-middle. To harden
/// to full verification, supply Supabase's CA certificate instead.
#[derive(Debug)]
struct NoCertVerification(rustls::crypto::CryptoProvider);

impl rustls::client::danger::ServerCertVerifier for NoCertVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn category_label(category: RollCategory) -> &'static str {
    match category {
        RollCategory::Above => "ABOVE AVERAGE",
        RollCategory::Average => "AVERAGE",
        RollCategory::Below => "BELOW AVERAGE",
    }
}

fn build_notation(results: &[SessionDieResult], modifier: i64) -> String {
    let die_order = ["d4", "d6", "d8", "d10", "d12", "d20", "d100"];
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for r in results {
        *counts.entry(r.die.as_str()).or_insert(0) += 1;
    }

    let mut parts: Vec<String> = Vec::new();
    for &die in &die_order {
        if let Some(&count) = counts.get(die) {
            if count > 0 {
                let notation_die = if die == "d100" { "d%" } else { die };
                if count == 1 {
                    parts.push(notation_die.to_string());
                } else {
                    parts.push(format!("{}{}", count, notation_die));
                }
            }
        }
    }

    let mut notation = parts.join("+");
    if modifier > 0 {
        notation.push_str(&format!("+{}", modifier));
    } else if modifier < 0 {
        notation.push_str(&format!("{}", modifier));
    }
    if notation.is_empty() {
        notation = "0".to_string();
    }
    notation
}

fn pct(n: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        100.0 * n as f64 / total as f64
    }
}

/// Fetches all rolls created on `date` (UTC) directly from the Supabase Postgres
/// database, connecting via the `DATABASE_URL` environment variable.
///
/// The table name defaults to `rolls` but can be overridden with
/// `SUPABASE_ROLLS_TABLE`. `date` must be a `YYYY-MM-DD` string; Postgres casts
/// it to a `date` and selects the half-open window `[date, date + 1 day)`.
fn fetch_from_supabase(date: &str) -> Vec<RollRecord> {
    use postgres::Client;
    use tokio_postgres_rustls::MakeRustlsConnect;

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL environment variable not set");
    let table = std::env::var("SUPABASE_ROLLS_TABLE").unwrap_or_else(|_| "dice_rolls".to_string());

    let provider = rustls::crypto::ring::default_provider();
    let tls_config = rustls::ClientConfig::builder_with_provider(provider.clone().into())
        .with_safe_default_protocol_versions()
        .expect("could not configure TLS protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(NoCertVerification(provider)))
        .with_no_client_auth();
    let connector = MakeRustlsConnect::new(tls_config);
    let mut client = Client::connect(&database_url, connector).unwrap_or_else(|e| {
        let mut msg = format!("could not connect to database: {}", e);
        let mut src = std::error::Error::source(&e);
        while let Some(s) = src {
            msg.push_str(&format!("\n  caused by: {}", s));
            src = s.source();
        }
        panic!("{}", msg);
    });

    // Table name can't be bound as a parameter, so it's interpolated; the date
    // is passed as a bound parameter ($1) to keep the query injection-safe.
    let query = format!(
        "SELECT display_name, modifier::int8 AS modifier, results, \
                total::int8 AS total, label \
         FROM {} \
         WHERE created_at >= ($1::text)::date \
           AND created_at < (($1::text)::date + interval '1 day') \
         ORDER BY created_at ASC",
        table,
    );

    let rows = client.query(&query, &[&date]).unwrap_or_else(|e| {
        let mut msg = format!("query failed: {}", e);
        let mut src = std::error::Error::source(&e);
        while let Some(s) = src {
            msg.push_str(&format!("\n  caused by: {}", s));
            src = s.source();
        }
        panic!("{}", msg);
    });

    rows.iter()
        .map(|row| {
            let results: serde_json::Value = row.get("results");
            RollRecord {
                display_name: row.get("display_name"),
                modifier: row.get("modifier"),
                results: serde_json::from_value(results).expect("could not parse 'results' column"),
                total: row.get("total"),
                label: row.get("label"),
            }
        })
        .collect()
}

fn main() {
    // Load variables from a local .env file (e.g. DATABASE_URL) if present.
    // Missing file is fine; real environment variables still take precedence.
    dotenvy::dotenv().ok();

    // Optional first argument: a YYYY-MM-DD date. When given, rolls are fetched
    // from Supabase for that single day; otherwise the local rolls.json is used.
    let date_arg = std::env::args().nth(1);

    let rolls: Vec<RollRecord> = match date_arg.as_deref() {
        Some(date) => {
            eprintln!("fetching rolls for {} from Supabase...", date);
            fetch_from_supabase(date)
        }
        None => {
            let json_path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/rolls.json");
            let data = std::fs::read_to_string(json_path).expect("could not read rolls.json");
            serde_json::from_str(&data).expect("could not parse rolls.json")
        }
    };

    let out_path = concat!(env!("CARGO_MANIFEST_DIR"), "/session_analysis_output.txt");
    let file =
        std::fs::File::create(out_path).expect("could not create session_analysis_output.txt");
    let mut out = BufWriter::new(file);

    let gm_name = "rbuckwild";

    let mut player_counts = [0usize; 3]; // [above, average, below]
    let mut gm_counts = [0usize; 3];
    let mut per_player: HashMap<String, [usize; 3]> = HashMap::new();
    let mut player_order: Vec<String> = Vec::new();

    writeln!(out, "\n=== SESSION ROLL ANALYSIS ===").unwrap();
    match date_arg.as_deref() {
        Some(date) => writeln!(
            out,
            "Source: Supabase  Date: {}  ({} rolls)\n",
            date,
            rolls.len()
        )
        .unwrap(),
        None => writeln!(out, "Source: rolls.json  ({} rolls)\n", rolls.len()).unwrap(),
    }
    writeln!(
        out,
        "{:<8} {:<14} {:<22} {:<14} {:>5}  {:>6}  {:>6}  {}",
        "ROLE", "PLAYER", "LABEL", "NOTATION", "TOTAL", "MEAN", "Z", "CATEGORY"
    )
    .unwrap();
    writeln!(out, "{}", "-".repeat(100)).unwrap();

    for roll in &rolls {
        let is_gm = roll.display_name == gm_name;
        let notation = build_notation(&roll.results, roll.modifier);
        let label = roll.label.as_deref().unwrap_or("—");
        let role = if is_gm { "GM" } else { "Player" };

        match distribution(&notation) {
            Ok(dist) => {
                let pos = dist.position_of(roll.total);
                let category = pos.category;

                writeln!(
                    out,
                    "{:<8} {:<14} {:<22} {:<14} {:>5}  {:>6.1}  {:>+6.2}  {}",
                    role,
                    roll.display_name,
                    label,
                    notation,
                    roll.total,
                    pos.mean,
                    pos.z_score,
                    category_label(category),
                )
                .unwrap();

                let bucket = match category {
                    RollCategory::Above => 0,
                    RollCategory::Average => 1,
                    RollCategory::Below => 2,
                };
                if is_gm {
                    gm_counts[bucket] += 1;
                } else {
                    player_counts[bucket] += 1;
                    let entry = per_player
                        .entry(roll.display_name.clone())
                        .or_insert_with(|| {
                            player_order.push(roll.display_name.clone());
                            [0usize; 3]
                        });
                    entry[bucket] += 1;
                }
            }
            Err(e) => {
                writeln!(
                    out,
                    "{:<8} {:<14} {:<22} {:<14} {:>5}  [distribution error: {}]",
                    role, roll.display_name, label, notation, roll.total, e,
                )
                .unwrap();
            }
        }
    }

    let p_total = player_counts.iter().sum::<usize>();
    let g_total = gm_counts.iter().sum::<usize>();

    writeln!(out, "\n{}", "=".repeat(100)).unwrap();
    writeln!(out, "\n=== TOTALS ===\n").unwrap();

    writeln!(out, "PLAYERS  ({} rolls):", p_total).unwrap();
    writeln!(
        out,
        "  Above average:  {:3}  ({:.1}%)",
        player_counts[0],
        pct(player_counts[0], p_total)
    )
    .unwrap();
    writeln!(
        out,
        "  Average:        {:3}  ({:.1}%)",
        player_counts[1],
        pct(player_counts[1], p_total)
    )
    .unwrap();
    writeln!(
        out,
        "  Below average:  {:3}  ({:.1}%)",
        player_counts[2],
        pct(player_counts[2], p_total)
    )
    .unwrap();

    writeln!(out, "\nGM / {} ({} rolls):", gm_name, g_total).unwrap();
    writeln!(
        out,
        "  Above average:  {:3}  ({:.1}%)",
        gm_counts[0],
        pct(gm_counts[0], g_total)
    )
    .unwrap();
    writeln!(
        out,
        "  Average:        {:3}  ({:.1}%)",
        gm_counts[1],
        pct(gm_counts[1], g_total)
    )
    .unwrap();
    writeln!(
        out,
        "  Below average:  {:3}  ({:.1}%)",
        gm_counts[2],
        pct(gm_counts[2], g_total)
    )
    .unwrap();

    writeln!(out, "\n{}", "=".repeat(100)).unwrap();
    writeln!(out, "\n=== PER-PLAYER BREAKDOWN ===\n").unwrap();

    for name in &player_order {
        let counts = &per_player[name];
        let total = counts.iter().sum::<usize>();
        writeln!(out, "{} ({} rolls):", name, total).unwrap();
        writeln!(
            out,
            "  Above average:  {:3}  ({:.1}%)",
            counts[0],
            pct(counts[0], total)
        )
        .unwrap();
        writeln!(
            out,
            "  Average:        {:3}  ({:.1}%)",
            counts[1],
            pct(counts[1], total)
        )
        .unwrap();
        writeln!(
            out,
            "  Below average:  {:3}  ({:.1}%)",
            counts[2],
            pct(counts[2], total)
        )
        .unwrap();
        writeln!(out).unwrap();
    }

    eprintln!("wrote {}", out_path);
}
