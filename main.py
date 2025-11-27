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
    "user-agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36",
}

response = requests.get(url, headers=headers)
print(response.status_code)

data = response.json()

# print(job_data_data.keys())
# print(title)


def get_title(jobs):
    print(jobs["data"]["title"])


def get_jobs(
    raw_json,
):  # gets each job from the json and then calls get title where it prints the title to the terminal
    for job in raw_json["jobs"]:
        get_title(job)


# get_title(data["jobs"])
get_jobs(data)


# print(json.dumps(jobs, indent=2))

# print(json.dumps(data, indent=2))


# print(data.keys())
# print(json.loads(data))
