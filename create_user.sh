#!/bin/bash
# Script to create admin user non-interactively

echo "test@example.com
admin
password123" | ./target/release/glimpser bootstrap
