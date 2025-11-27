import requests
import json


limit = 1  # can be 5, 10, 25, 100
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
# print(response.text)


data = response.json()
jobs = data["jobs"]
# jobs at index one is a dict of "data"
job_data = jobs[0]
job_data_data = job_data["data"]
title = job_data_data["title"]


# print(job_data_data.keys())
print(title)


# print(json.dumps(jobs, indent=2))

# for i, key in enumerate(jobs):
# print(i, key)


# print(response)
# print(data["path"])

# print(json.dumps(data, indent=2))

# print(data.keys())
# print(json.loads(data))
