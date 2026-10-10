"""A number no model can guess: only running this gives it."""
import sys

name = sys.argv[1].strip().lower()
print(sum(ord(c) * (i + 7) for i, c in enumerate(name)) * 7919 % 9000 + 1000)
