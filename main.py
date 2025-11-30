import os
import requests
import smtplib
from dotenv import load_dotenv

load_dotenv()

INTERACTIVE = False

scored = []


def loop_pages(n, limit, *terms):
    scored.clear()
    while n > 0:
        url = (
            "https://careers.heb.com/api/jobs"
            f"?page={n}"
            f"&sortBy=relevance"
            "&categories=Store%20Operations"
            "&tags1=Part%20Time"
            "&internal=false"
            f"&limit={limit}"
        )
        headers = {
            "accept": "application/json, text/plain, */*",
            "accept-language": "en-US,en;q=0.9",
            "priority": "u=1, i",
            "referer": f"https://careers.heb.com/jobs?page={n}&sortBy=relevance&categories=Store%20Operations&tags1=Part%20Time&limit={limit}",
            "sec-ch-ua": '"Chromium";v="142", "Google Chrome";v="142", "Not_A Brand";v="99"',
            "sec-ch-ua-mobile": "?0",
            "sec-ch-ua-platform": '"Linux"',
            "sec-fetch-dest": "empty",
            "sec-fetch-mode": "cors",
            "sec-fetch-site": "same-origin",
        }
        response = requests.get(url, headers=headers)
        print(f"Status code: {response.status_code}")
        n -= 1

        get_jobs(response.json(), *terms)

    scored.sort(reverse=True, key=lambda x: x[0])
    return scored


def score_job(job, *terms):
    data = job.get("data", {})

    title = data.get("title", "") or ""
    summary = data.get("short_description", "") or ""
    location = data.get("location", "") or ""
    full_location = data.get("full_location", "") or ""
    short_location = data.get("short_location", "") or ""

    blob = f"{title} {summary} {location} {full_location} {short_location}".lower()

    score = 0
    for term in terms:
        t = term.lower()
        if t in title.lower():
            score += 5
        if t in summary.lower():
            score += 3
        if t in blob:
            score += 1

    return score


def get_title_and_link(job):
    d = job["data"]
    return (d.get("title"), f"https://careers.heb.com/jobs/{d.get('slug')}")


def get_jobs(raw_json, *terms):
    for job in raw_json["jobs"]:
        s = score_job(job, *terms)
        if s > 0:
            scored.append((s, get_title_and_link(job)))


def print_scored(scored_list, k):
    num = int(k)
    for score, (title, link) in scored_list[:num]:
        print(score, title, link)


if INTERACTIVE:
    limit = input("Enter limit (max is 100): ")
    page = input("Enter page: ")

    raw_terms = input("Enter term(s) to filter by (space-separated): ")
    terms = tuple(t for t in raw_terms.split() if t)

    output_count = int(input("How many would you like to output? "))
else:
    raw_terms = os.getenv("JOB_TERMS") or "curbie estore 52 san antonio"
    terms = tuple(t.strip() for t in raw_terms.replace(",", " ").split() if t)

    limit = os.getenv("JOB_LIMIT") or "100"
    page = os.getenv("JOB_PAGE_START") or "3"
    output_count = int(os.getenv("JOB_OUTPUT_COUNT") or "10")


email_id = os.getenv("EMAIL_ID")
email_pword = os.getenv("EMAIL_PWORD")

graded = loop_pages(int(page), limit, *terms)

if not graded:
    print("NO JOBS FOUND!")
else:
    print("===GOT THE JOBS===")
    if INTERACTIVE:
        print_scored(graded, output_count)
    else:
        lines = []
        for score, (title, link) in graded[:output_count]:
            lines.append(f"{score} | {title} | {link}")

        message = f"Top {output_count} jobs:\n\n" + "\n".join(lines)

        s = smtplib.SMTP("smtp.gmail.com", 587)
        s.starttls()
        s.login(str(email_id), str(email_pword))
        s.sendmail(str(email_id), str(email_id), message)
        s.quit()
