import requests
import json


limit = 100  # can be 5, 10, 25, 100
page = 1
url = (
    "https://careers.heb.com/api/jobs"
    f"?page={page}"
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
    "referer": f"https://careers.heb.com/jobs?page={page}&sortBy=relevance&categories=Store%20Operations&tags1=Part%20Time&limit={limit}",
    "sec-ch-ua": '"Chromium";v="142", "Google Chrome";v="142", "Not_A Brand";v="99"',
    "sec-ch-ua-mobile": "?0",
    "sec-ch-ua-platform": '"Linux"',
    "sec-fetch-dest": "empty",
    "sec-fetch-mode": "cors",
    "sec-fetch-site": "same-origin",
}

response = requests.get(url, headers=headers)
print(response.status_code)

data = response.json()


def score_job(job, term):
    data = job.get("data", {})
    term = term.lower()

    title = data.get("title", "") or ""
    summary = data.get("short_description", "") or ""
    location = data.get("location", "") or ""

    blob = f"{title} {summary} {location}".lower()

    score = 0
    if term in title.lower():
        score += 5
    if term in summary.lower():
        score += 3
    if term in blob:
        score += 1

    return score


def get_title_and_link(job):
    d = job["data"]
    return (d.get("title"), d.get("apply_url"))


def get_jobs(raw_json, term):
    scored = []

    for job in raw_json["jobs"]:
        s = score_job(job, term)
        if s > 0:
            scored.append((s, get_title_and_link(job)))

    scored.sort(reverse=True, key=lambda x: x[0])

    for score, (title, link) in scored:
        print(score, title, link)


get_jobs(data, "service")


# print(json.dumps(jobs, indent=2))

# print(json.dumps(data, indent=2))


# print(data.keys())
# print(json.loads(data))
