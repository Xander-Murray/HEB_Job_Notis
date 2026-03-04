import argparse
import json
import os
import smtplib
from datetime import date
from email.message import EmailMessage

import requests
from dotenv import load_dotenv

load_dotenv()


def loop_pages(n, limit, *terms):
    scored = []

    s = requests.Session()
    s.headers.update(
        {
            "accept": "application/json, text/plain, */*",
            "accept-language": "en-US,en;q=0.9",
            "user-agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36",
            "origin": "https://heb.jibeapply.com",
            "referer": "https://heb.jibeapply.com/",
        }
    )

    keywords = " ".join(terms)

    while n > 0:
        url = "https://heb.jibeapply.com/api/jobs"
        params = {
            "keywords": keywords,
            "sortBy": "relevance",
            "page": n,
            "internal": "true",
            "limit": limit,
        }

        resp = s.get(url, params=params, timeout=20)
        ct = resp.headers.get("content-type", "")
        print(f"page={n} status={resp.status_code} ct={ct} bytes={len(resp.content)}")

        if resp.status_code != 200:
            print("Non-200 response:", resp.text[:200])
            n -= 1
            continue

        try:
            data = resp.json()
        except Exception as e:
            print("JSON decode failed:", e)
            print(resp.text[:200])
            n -= 1
            continue

        if not data.get("jobs"):
            print(f"page={n} returned no jobs, stopping early")
            break

        scored.extend(get_jobs(data, *terms))
        n -= 1

    scored.sort(reverse=True, key=lambda x: x[0])
    return scored


def score_job(job, *terms):
    data = job.get("data", {})

    title = data.get("title", "") or ""
    summary = data.get("short_description", "") or ""
    location = data.get("location", "") or ""
    full_location = data.get("full_location", "") or ""
    short_location = data.get("short_location", "") or ""

    # Blob only covers location fields to avoid double-scoring title/summary terms
    location_blob = f"{location} {full_location} {short_location}".lower()

    score = 0
    for term in terms:
        t = term.lower()
        if t in title.lower():
            score += 5
        if t in summary.lower():
            score += 3
        if t in location_blob:
            score += 1

    return score


def get_title_and_link(job):
    d = job.get("data", {})
    slug = d.get("slug") or ""
    if not slug:
        return None
    return (d.get("title"), f"https://careers.heb.com/jobs/{slug}")


def get_jobs(raw_json, *terms):
    results = []
    for job in raw_json.get("jobs", []):
        s = score_job(job, *terms)
        if s > 0:
            title_link = get_title_and_link(job)
            if title_link:
                results.append((s, title_link))
    return results


def print_scored(scored_list, k):
    for score, (title, link) in scored_list[:k]:
        print(score, title, link)


def build_html_email(graded, output_count, recipient_email):
    today = date.today().isoformat()
    rows = ""
    for score, (title, link) in graded[:output_count]:
        rows += (
            f"<tr>"
            f"<td style='padding:4px 8px;border:1px solid #ddd;'>{score}</td>"
            f"<td style='padding:4px 8px;border:1px solid #ddd;'><a href='{link}'>{title}</a></td>"
            f"</tr>\n"
        )

    html = f"""<html><body>
<h2>HEB Job Alerts &mdash; {today}</h2>
<p>Top {output_count} jobs for <b>{recipient_email}</b>:</p>
<table style='border-collapse:collapse;font-family:sans-serif;font-size:14px;'>
<thead><tr>
  <th style='padding:4px 8px;border:1px solid #ddd;background:#f5f5f5;'>Score</th>
  <th style='padding:4px 8px;border:1px solid #ddd;background:#f5f5f5;'>Title</th>
</tr></thead>
<tbody>
{rows}</tbody>
</table>
</body></html>"""
    return html, today


def send_email(email_id, email_pword, to_addr, subject, html_body):
    msg = EmailMessage()
    msg["Subject"] = subject
    msg["From"] = email_id
    msg["To"] = to_addr
    msg.set_content("Please view this email in an HTML-capable client.")
    msg.add_alternative(html_body, subtype="html")

    try:
        with smtplib.SMTP("smtp.gmail.com", 587) as s:
            s.starttls()
            s.login(email_id, email_pword)
            s.send_message(msg)
        print(f"Email sent to {to_addr}")
    except smtplib.SMTPException as e:
        print(f"SMTP error sending to {to_addr}: {e}")


def run_automated(email_id, email_pword, limit, page):
    config_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "config.json")
    if os.path.exists(config_path):
        with open(config_path) as f:
            profiles = json.load(f)
    else:
        # Fall back to env vars for single-user mode
        raw_terms = os.getenv("JOB_TERMS") or "curbie estore san antonio"
        terms = [t.strip() for t in raw_terms.replace(",", " ").split() if t]
        output_count = int(os.getenv("JOB_OUTPUT_COUNT") or "20")
        profiles = [{"email": email_id, "terms": terms, "output_count": output_count}]

    for profile in profiles:
        to_addr = profile["email"]
        terms = tuple(profile["terms"])
        output_count = int(profile.get("output_count", 20))

        print(f"\n--- Processing profile: {to_addr} | terms: {terms} ---")
        graded = loop_pages(int(page), limit, *terms)

        if not graded:
            print(f"NO JOBS FOUND for {to_addr}")
            continue

        print(f"Found {len(graded)} matching jobs for {to_addr}")
        html_body, today = build_html_email(graded, output_count, to_addr)
        subject = f"HEB Job Alerts - {today} ({to_addr})"
        send_email(email_id, email_pword, to_addr, subject, html_body)


def run_interactive(limit, page):
    raw_terms = input("Enter term(s) to filter by (space-separated): ")
    terms = tuple(t for t in raw_terms.split() if t)
    output_count = int(input("How many would you like to output? "))

    graded = loop_pages(int(page), limit, *terms)

    if not graded:
        print("NO JOBS FOUND!")
    else:
        print("===GOT THE JOBS===")
        print_scored(graded, output_count)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="HEB Job Alerts")
    parser.add_argument("--interactive", action="store_true", help="Prompt for inputs instead of using config/env")
    args = parser.parse_args()

    email_id = os.getenv("EMAIL_ID")
    email_pword = os.getenv("EMAIL_PWORD")

    limit = os.getenv("JOB_LIMIT") or "100"
    page = os.getenv("JOB_PAGE_START") or "4"

    if args.interactive:
        limit = input(f"Enter limit (max is 100) [{limit}]: ") or limit
        page = input(f"Enter page [{page}]: ") or page
        run_interactive(limit, page)
    else:
        if not email_id or not email_pword:
            print("ERROR: EMAIL_ID and EMAIL_PWORD must be set for automated mode")
            raise SystemExit(1)
        run_automated(email_id, email_pword, limit, page)
