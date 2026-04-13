#!/usr/bin/env python3
"""
Stdin/stdout bridge for the graph search agent.

Protocol:
- Reads one JSON object per line from stdin (observation or control message)
- Writes one JSON object per line to stdout (actions)

Messages:
- {"type": "reset"} -> resets agent state, no response
- {"type": "observation", ...} -> responds with {"actions": [...]}
"""

import json
import sys

from agent import GraphSearchAgent
from observation import parse_observation


def main():
    agent = GraphSearchAgent(name="GraphSearch")

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue

        msg_type = msg.get("type")

        if msg_type == "reset":
            agent.reset()
            continue

        if msg_type == "observation":
            obs = parse_observation(msg)
            actions = agent.decide(obs)
            response = json.dumps({"actions": actions})
            sys.stdout.write(response + "\n")
            sys.stdout.flush()
            continue


if __name__ == "__main__":
    main()
