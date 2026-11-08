FROM python:3.12-slim
WORKDIR /app
COPY magnus_teams ./magnus_teams
ENV MAGNUS_TEAMS_STORE=/data/events.jsonl PORT=8080
EXPOSE 8080
CMD ["python", "-m", "magnus_teams.server"]
