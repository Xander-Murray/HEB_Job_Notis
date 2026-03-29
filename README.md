# HEB Job Alerts

Python script that scrapes H-E-B job postings, scores them by keyword relevance, and emails the top matches daily via GitHub Actions.

## How It Works

1. Fetches jobs from HEB's careers API (`heb.jibeapply.com/api/jobs`) with paginated requests
2. Scores each job by keyword matches -- title hits are weighted highest (+5), then summary (+3), then location (+1)
3. Sends an HTML email with the top-ranked results to each configured recipient

## Setup

```bash
pip install -r requirements.txt
```

### Environment Variables

| Variable | Description | Default |
|---|---|---|
| `EMAIL_ID` | Gmail address for sending alerts | required |
| `EMAIL_PWORD` | Gmail app password ([create one here](https://myaccount.google.com/apppasswords)) | required |
| `JOB_LIMIT` | Max results per API page (1-100) | `100` |
| `JOB_PAGE_START` | Number of pages to fetch | `10` |

### Multi-User Config

Add recipients and per-user search terms in `config.json`:

```json
[
  {
    "email": "user@example.com",
    "terms": ["curbie", "estore", "san", "antonio"],
    "output_count": 20
  }
]
```

If no `config.json` exists, falls back to `JOB_TERMS` and `JOB_OUTPUT_COUNT` env vars for single-user mode.

## Usage

**Automated mode** (used by GitHub Actions):
```bash
python main.py
```

**Interactive mode** (for testing keywords locally):
```bash
python main.py --interactive
```

## GitHub Actions

Runs daily at 23:00 UTC via `.github/workflows/actions.yml`. Can also be triggered manually with `workflow_dispatch`.

Set `EMAIL_ID` and `EMAIL_PWORD` as repository secrets, and `JOB_LIMIT` / `JOB_PAGE_START` as repository variables.
