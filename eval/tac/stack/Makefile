SHELL = /bin/bash

HOSTNAME?=localhost
FILE_SERVER_PORT?=8081
GITLAB_PORT?=8929

.PHONY: init start-all stop-all start-file-server stop-file-server start-gitlab stop-gitlab rebuild-gitlab reset-gitlab gitlab-ready \
		start-owncloud stop-owncloud reset-owncloud \
		start-rocketchat stop-rocketchat rm-rocketchat rm-rocketchat-volume reset-rocketchat backup-rocketchat

init:
	$(eval export HOSTNAME)
	$(eval export GITLAB_PORT)
	@echo "Initialization done."

start-all: init
	@echo "stopping everything..."
	-docker compose -p theagentcompany down -v
	@echo "stopping plane..."
	-cd /plane && make stop-plane
	-cd /plane && make rm-plane-volume
	@echo "starting everything..."
	-docker compose -p theagentcompany up -d
	-$(MAKE) gitlab-ready
	@echo "starting plane..."
	-cd /plane && make start-plane
	-make reset-rocketchat

# reset stops the gitlab container, destroys and starts again
# any changes to gitlab will disappear, and the container will start
# in a clean state (with pre-populated data)
reset-gitlab: init
	@echo "stopping existing gitlab instance..."
	docker compose -p theagentcompany stop gitlab
	@echo "remove gitlab container..."
	docker compose -p theagentcompany rm -f -v gitlab
	@echo "start gitlab from clean state..."
	docker compose -p theagentcompany up gitlab -d
	$(MAKE) gitlab-ready

# Magnus override (eval/tac/README.md 1.2). The image's seeded access tokens
# (root-token, used by task setup and graders) expired on 2025-11-18; a
# reset brings them back expired, so extend them once the database is up.
# A first boot that fails ("runsv not running") is started again once.
gitlab-ready:
	@restarted=0; for i in $$(seq 1 120); do \
	  if [ "$$(docker inspect -f '{{.State.Running}}' gitlab)" != true ]; then \
	    if [ $$restarted = 1 ]; then echo "gitlab exited twice"; exit 1; fi; \
	    echo "gitlab exited on first boot; starting it again"; docker start gitlab >/dev/null; restarted=1; \
	  elif docker exec gitlab gitlab-psql -tAc "UPDATE personal_access_tokens SET expires_at = CURRENT_DATE + 365 WHERE revoked = false AND expires_at < CURRENT_DATE + 30" 2>/dev/null | grep -q .; then \
	    echo "gitlab tokens extended"; exit 0; \
	  fi; sleep 5; \
	done; echo "gitlab database never came up"; exit 1

# Sotopia Redis
reset-sotopia-redis: init
	@echo "stopping existing sotopia redis instance..."
	docker compose -p theagentcompany stop redis-stack redis-stack-npc-data-population
	@echo "remove sotopia redis container..."
	docker compose -p theagentcompany rm -f redis-stack redis-stack-npc-data-population
	@echo "start sotopia redis from clean state..."
	docker compose -p theagentcompany up redis-stack redis-stack-npc-data-population -d

# owncloud
reset-owncloud: init
	@echo "stopping existing owncloud instance..."
	docker compose -p theagentcompany stop owncloud owncloud-collabora
	@echo "remove owncloud container..."
	docker compose -p theagentcompany rm -f owncloud owncloud-collabora
	@echo "start owncloud from clean state..."
	docker compose -p theagentcompany up owncloud owncloud-collabora -d

# Inside the container
# RocketChat
reset-rocketchat:
	cd /rocketchat && ./restore.sh

# Plane
reset-plane:
	cd /plane && make reset-plane