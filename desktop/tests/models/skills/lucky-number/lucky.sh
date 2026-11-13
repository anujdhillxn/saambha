#!/usr/bin/env bash
# A number no model can guess: only running this gives it.
name=$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
sum=0
for ((i = 0; i < ${#name}; i++)); do
    sum=$((sum + $(printf '%d' "'${name:i:1}") * (i + 7)))
done
echo $((sum * 7919 % 9000 + 1000))
