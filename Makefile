PROJECT_NAME=$(shell basename "$(PWD)")

## help: Get more info on make commands.
help: Makefile
	@echo " Choose a command run in "$(PROJECT_NAME)":"
	@sed -n 's/^##//p' $< | sort | column -t -s ':' | sed -e 's/^/ /'
.PHONY: help

## check-dependencies: Check if all dependencies are installed.
check-dependencies:
	@echo "--> Checking if all dependencies are installed"
	@if command -v cargo >/dev/null 2>&1; then \
		echo "cargo is installed."; \
	else \
		echo "Error: cargo is not installed. Please install Rust."; \
		exit 1; \
	fi
	@if command -v forge >/dev/null 2>&1; then \
		echo "foundry is installed."; \
	else \
		echo "Error: forge is not installed. Please install Foundry."; \
		exit 1; \
	fi
	@if command -v cargo prove >/dev/null 2>&1; then \
		echo "cargo prove is installed."; \
	else \
		echo "Error: succinct is not installed. Please install SP1."; \
		exit 1; \
	fi
	@echo "All dependencies are installed."
.PHONY: check-dependencies

## start: Start all Docker containers for the demo.
start:
	@echo "--> Starting all Docker containers"
	@docker compose up --detach
.PHONY: start

## stop: Stop all Docker containers and remove volumes.
stop:
	@echo "--> Stopping all Docker containers"
	@docker compose down -v
.PHONY: stop
