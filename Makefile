.PHONY: all
all: ecr-login build push

SHA := $(shell git rev-parse HEAD)
DOCKER_IMAGE_NAME := prometheus-gravel-gateway

ecr-login:
	aws ecr get-login-password --profile $(AWS_PROFILE) --region $(AWS_REGION) | docker login --username AWS --password-stdin $(AWS_ACCOUNT_ID).dkr.ecr.$(AWS_REGION).amazonaws.com

build:
	docker build -t $(DOCKER_IMAGE_NAME):$(SHA) .

push:
	docker push $(DOCKER_IMAGE_NAME):$(SHA)

