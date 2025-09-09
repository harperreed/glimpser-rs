# ABOUTME: Makefile for managing Docker operations and development workflow
# ABOUTME: Provides convenient commands for building, running, and maintaining the application

.PHONY: help build up down logs clean dev prod restart health

# Default target
help:
	@echo "Glimpser Docker Management"
	@echo "=========================="
	@echo "Production commands:"
	@echo "  make build     - Build all Docker images"
	@echo "  make up        - Start production services"
	@echo "  make down      - Stop production services"
	@echo "  make restart   - Restart production services"
	@echo ""
	@echo "Development commands:"
	@echo "  make dev       - Start development services with hot-reload"
	@echo "  make dev-down  - Stop development services"
	@echo ""
	@echo "Utility commands:"
	@echo "  make logs      - Show service logs"
	@echo "  make health    - Check service health"
	@echo "  make clean     - Clean up Docker resources"
	@echo "  make reset     - Reset everything (clean + rebuild)"

# Production targets
build:
	@echo "Building production images..."
	docker-compose build --parallel

up: build
	@echo "Starting production services..."
	docker-compose up -d

down:
	@echo "Stopping production services..."
	docker-compose down

restart: down up
	@echo "Services restarted"

# Development targets
dev:
	@echo "Starting development services..."
	docker-compose -f docker-compose.dev.yml up --build

dev-down:
	@echo "Stopping development services..."
	docker-compose -f docker-compose.dev.yml down

# Utility targets
logs:
	docker-compose logs -f --tail=100

dev-logs:
	docker-compose -f docker-compose.dev.yml logs -f --tail=100

health:
	@echo "Checking service health..."
	@docker-compose ps
	@echo ""
	@echo "Backend health:"
	@curl -s http://localhost:3000/health || echo "Backend not responding"
	@echo ""
	@echo "Frontend health:"
	@curl -s http://localhost:3001 > /dev/null && echo "Frontend OK" || echo "Frontend not responding"
	@echo ""
	@echo "Nginx health:"
	@curl -s http://localhost/health || echo "Nginx not responding"

clean:
	@echo "Cleaning up Docker resources..."
	docker-compose down --remove-orphans
	docker-compose -f docker-compose.dev.yml down --remove-orphans
	docker system prune -f
	docker volume prune -f

reset: clean
	@echo "Rebuilding everything from scratch..."
	docker-compose build --no-cache --parallel
	docker-compose up -d

# Database operations
db-reset:
	@echo "Resetting database..."
	docker-compose stop database
	docker volume rm glimpser-postgres-data || true
	docker-compose up -d database

db-migrate:
	@echo "Running database migrations..."
	docker-compose exec backend ./glimpser migrate

# Monitoring
stats:
	docker stats

top:
	docker-compose top
