import requests
import json


scored = []


def loop_pages(n, term):
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
            "referer": f"https://careers.heb.com/jobs?page={page}&sortBy=relevance&categories=Store%20Operations&tags1=Part%20Time&limit={limit}",
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

        get_jobs(response.json(), term)

    scored.sort(reverse=True, key=lambda x: x[0])
    print("----GOT THE JOBS----")
    return scored


def score_job(job, term):
    data = job.get("data", {})
    term = term.lower()

    title = data.get("title", "") or ""
    summary = data.get("short_description", "") or ""
    location = data.get("location", "") or ""
    full_location = data.get("full_location", "") or ""
    short_location = data.get("short_location", "") or ""

    blob = f"{title} {summary} {location} {full_location} {short_location}".lower()

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
    for job in raw_json["jobs"]:
        s = score_job(job, term)
        if s > 0:
            scored.append((s, get_title_and_link(job)))


def print_scored(scored):
    for score, (title, link) in scored:
        print(score, title, link)


limit = input("Enter limit:(max is 100) ")  # can be 5, 10, 25, 100
page = input("Enter page: ")

search_term = input("Enter a term to filter by: ")

graded = loop_pages(int(page), search_term)
print_scored(graded)


# print(json.dumps(jobs, indent=2))

# print(json.dumps(data, indent=2))


# print(data.keys())
# print(json.loads(data))
