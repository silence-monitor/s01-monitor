#!/bin/bash
# Starts the CAN dump (listen only). Close this window or press Ctrl+C to stop. Safe to delete.
cd "$(dirname "$0")" || exit 1
python3 scripts/can_dump.py
echo
echo "Dump stopped. You can close this window."
