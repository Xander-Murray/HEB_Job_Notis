//! Building and sending the alert email.

use anyhow::{Context, Result};
use lettre::{
    Message, SmtpTransport, Transport,
    message::{Mailbox, MultiPart},
    transport::smtp::authentication::Credentials,
};

use crate::matcher::{Hit, Role, StoreMatch};

/// Gmail login. The password is a Gmail *app password*, not the real one.
pub struct Creds {
    pub user: String,
    pub password: String,
}

/// Subject line, e.g. "HEB: 2 new openings at your stores".
pub fn subject(n: usize) -> String {
    format!("HEB: {}", count_phrase(n))
}

/// "1 new opening at your stores" / "N new openings at your stores".
fn count_phrase(n: usize) -> String {
    let plural = if n == 1 { "" } else { "s" };
    format!("{n} new opening{plural} at your stores")
}

/// The display pieces of one job row, shared by the HTML and text versions.
struct Row<'a> {
    title: &'a str,
    link: String,
    store: &'a str,
    hours: &'static str,
    posted: &'a str,
    check_location: bool,
}

// Rust lesson: the `'a` here is a *lifetime*. It tells the compiler that a
// `Row` borrows strings from a `Hit` and must not outlive it. That's how
// Rust avoids copying every string while still guaranteeing no dangling
// references.
impl<'a> Row<'a> {
    fn from_hit(hit: &'a Hit) -> Row<'a> {
        let job = &hit.job;
        Row {
            title: &job.title,
            link: job.link(),
            store: job.location_name.as_deref().unwrap_or("Store not listed"),
            hours: if job.is_part_time() { "Part time" } else { "Full time" },
            // "2026-11-24T15:04:00+0000" -> "2026-11-24". `get(..10)` returns
            // None instead of panicking if the string is unexpectedly short.
            posted: job
                .posted_date
                .as_deref()
                .map(|d| d.get(..10).unwrap_or(d))
                .unwrap_or("unknown"),
            check_location: hit.store == StoreMatch::Street,
        }
    }
}

/// The two email sections, in display order. Empty ones are skipped.
fn sections(hits: &[Hit]) -> Vec<(&'static str, Vec<Row<'_>>)> {
    [(Role::EntryLevel, "Entry-level"), (Role::Other, "Other roles")]
        .into_iter()
        .map(|(role, heading)| {
            let rows = hits.iter().filter(|h| h.role == role).map(Row::from_hit).collect();
            (heading, rows)
        })
        .filter(|(_, rows): &(&str, Vec<Row>)| !rows.is_empty())
        .collect()
}

/// Make text safe to drop into HTML (so a title like "A & B" can't break
/// the markup).
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Render the email body as `(html, plain_text)`.
pub fn render(hits: &[Hit]) -> (String, String) {
    let sections = sections(hits);
    let summary = count_phrase(hits.len());

    // ---- Plain text version (for clients that don't show HTML) ----
    let mut text = format!("HEB Job Alerts: {summary}\n");
    for (heading, rows) in &sections {
        text.push_str(&format!("\n== {heading} ==\n"));
        for r in rows {
            let flag = if r.check_location { " [check location]" } else { "" };
            text.push_str(&format!(
                "\n{}{flag}\n  {} | {} | posted {}\n  {}\n",
                r.title, r.store, r.hours, r.posted, r.link
            ));
        }
    }
    text.push_str("\nApply quickly: store openings fill fast.\n");

    // ---- HTML version ----
    // Email clients ignore <style> blocks and modern CSS, so everything is
    // inline styles on simple tables.
    let mut body = String::new();
    for (heading, rows) in &sections {
        body.push_str(&format!(
            r#"<tr><td style="padding:20px 24px 6px;font:700 13px/1.2 Arial,sans-serif;letter-spacing:.08em;text-transform:uppercase;color:#E1251B;">{heading}</td></tr>"#
        ));
        for r in rows {
            let flag = if r.check_location {
                r#" <span style="display:inline-block;padding:2px 8px;margin-left:6px;border-radius:10px;background:#FFF1C2;color:#7A5200;font:700 11px Arial,sans-serif;">check location</span>"#
            } else {
                ""
            };
            body.push_str(&format!(
                r#"<tr><td style="padding:10px 24px;border-bottom:1px solid #EEE;">
<a href="{link}" style="font:700 16px/1.35 Arial,sans-serif;color:#1A1A1A;text-decoration:none;">{title}</a>{flag}
<div style="margin-top:4px;font:14px/1.4 Arial,sans-serif;color:#555;">{store} &middot; {hours} &middot; posted {posted}</div>
<div style="margin-top:6px;"><a href="{link}" style="font:700 13px Arial,sans-serif;color:#E1251B;">View &amp; apply &rarr;</a></div>
</td></tr>"#,
                link = escape_html(&r.link),
                title = escape_html(r.title),
                store = escape_html(r.store),
                hours = r.hours,
                posted = escape_html(r.posted),
            ));
        }
    }

    let html = format!(
        r#"<!doctype html><html><body style="margin:0;padding:0;background:#F4F1EC;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#F4F1EC;padding:16px 0;"><tr><td align="center">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width:600px;background:#FFFFFF;border-radius:8px;overflow:hidden;">
<tr><td style="background:#E1251B;padding:18px 24px;font:700 20px Arial,sans-serif;color:#FFFFFF;">HEB Job Alerts</td></tr>
<tr><td style="padding:16px 24px 0;font:16px/1.4 Arial,sans-serif;color:#1A1A1A;">{summary}. Apply quickly: store openings fill fast.</td></tr>
{body}
<tr><td style="padding:16px 24px;font:12px/1.4 Arial,sans-serif;color:#888;">Checked automatically every 2 hours. You only get an email when something new is posted.</td></tr>
</table></td></tr></table></body></html>"#
    );

    (html, text)
}

/// Send one email to every recipient via Gmail SMTP (STARTTLS, port 587).
pub fn send(creds: &Creds, to: &[String], subject: &str, html: &str, text: &str) -> Result<()> {
    // Rust lesson: `.parse::<Mailbox>()` uses the `FromStr` trait to turn a
    // string into a validated email address type. A typo in a secret becomes
    // a clear error here instead of a confusing SMTP failure later.
    let from: Mailbox = creds.user.parse().context("EMAIL_ID is not a valid address")?;
    let mut builder = Message::builder().from(from).subject(subject);
    for addr in to {
        let mailbox: Mailbox =
            addr.parse().with_context(|| format!("invalid recipient address {addr:?}"))?;
        builder = builder.to(mailbox);
    }
    // `alternative_plain_html` sends both versions; the mail app picks one.
    let message = builder
        .multipart(MultiPart::alternative_plain_html(text.to_string(), html.to_string()))
        .context("building email")?;

    let mailer = SmtpTransport::starttls_relay("smtp.gmail.com")
        .context("setting up Gmail SMTP")?
        .credentials(Credentials::new(creds.user.clone(), creds.password.clone()))
        .build();
    mailer.send(&message).context("sending email via Gmail")?;
    println!("email sent to {} recipient(s)", to.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::tests::job;

    fn hit(title: &str, role: Role, store: StoreMatch) -> Hit {
        Hit { job: job(title, Some("SA49-Potranco/SH 211")), store, role }
    }

    #[test]
    fn subject_singular_plural() {
        assert_eq!(subject(1), "HEB: 1 new opening at your stores");
        assert_eq!(subject(3), "HEB: 3 new openings at your stores");
    }

    #[test]
    fn render_sections_in_order() {
        let hits = [
            hit("Curbie", Role::EntryLevel, StoreMatch::Number(49)),
            hit("Pharmacy Technician", Role::Other, StoreMatch::Number(49)),
        ];
        let (html, text) = render(&hits);
        for body in [&html, &text] {
            let entry = body.find("Entry-level").expect("entry section");
            let other = body.find("Other roles").expect("other section");
            assert!(entry < other);
        }
    }

    #[test]
    fn render_omits_empty_section() {
        let hits = [hit("Curbie", Role::EntryLevel, StoreMatch::Number(49))];
        let (html, text) = render(&hits);
        assert!(!html.contains("Other roles"));
        assert!(!text.contains("Other roles"));
    }

    #[test]
    fn render_marks_street_matches() {
        let (html, text) = render(&[hit("Curbie", Role::EntryLevel, StoreMatch::Street)]);
        assert!(html.contains("check location"));
        assert!(text.contains("check location"));
        let (html, _) = render(&[hit("Curbie", Role::EntryLevel, StoreMatch::Number(49))]);
        assert!(!html.contains("check location"));
    }

    #[test]
    fn render_escapes_html() {
        let (html, text) = render(&[hit("A & B <x>", Role::Other, StoreMatch::Number(49))]);
        assert!(html.contains("A &amp; B &lt;x&gt;"));
        assert!(!html.contains("<x>"));
        assert!(text.contains("A & B <x>"));
    }

    #[test]
    fn render_row_details() {
        let mut h = hit("Curbie", Role::EntryLevel, StoreMatch::Number(49));
        h.job.slug = "250480".into();
        h.job.employment_type = Some("PART_TIME".into());
        let (html, text) = render(&[h]);
        for body in [&html, &text] {
            assert!(body.contains("https://careers.heb.com/jobs/250480"));
            assert!(body.contains("SA49-Potranco/SH 211"));
            assert!(body.contains("Part time"));
            assert!(body.contains("2026-11-24"));
        }
    }
}
