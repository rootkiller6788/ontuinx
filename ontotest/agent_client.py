"""
OntoRuntime Agent API client.

Sends tasks to the OntoRuntime server via the Reborn WebChat v2 API.
Waits for agent completion by polling thread status.
"""

import time
import uuid
import requests
from dataclasses import dataclass, field
from typing import Optional


@dataclass
class AgentTask:
    prompt: str
    requirements: list[str] = field(default_factory=list)


@dataclass
class AgentResult:
    task_id: str
    thread_id: str
    status: str  # "running" | "completed" | "failed" | "timeout"
    response_text: str = ""
    duration_seconds: float = 0.0
    error: Optional[str] = None


class AgentClient:
    """Client for OntoRuntime Reborn WebChat v2 API."""

    def __init__(self, base_url: str, token: str):
        self.base_url = base_url.rstrip("/")
        self.token = token
        self.headers = {
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        }

    def create_thread(self) -> Optional[str]:
        """Create a new conversation thread."""
        cid = str(uuid.uuid4())
        try:
            resp = requests.post(
                f"{self.base_url}/api/webchat/v2/threads",
                headers=self.headers,
                json={"client_action_id": cid},
                timeout=10,
            )
            if resp.status_code == 200:
                data = resp.json()
                return data.get("thread", {}).get("thread_id")
        except requests.RequestException:
            pass
        return None

    def send_message(self, thread_id: str, content: str) -> bool:
        """Send a user message to a thread. Returns True if accepted (200/202)."""
        cid = str(uuid.uuid4())
        try:
            resp = requests.post(
                f"{self.base_url}/api/webchat/v2/threads/{thread_id}/send_message",
                headers=self.headers,
                json={"client_action_id": cid, "content": content},
                timeout=15,
            )
            # 200 = synchronous response, 202 = accepted for async processing
            return resp.status_code in (200, 202)
        except requests.RequestException:
            return False

    def get_thread_status(self, thread_id: str) -> Optional[str]:
        """Get the current status of a thread."""
        try:
            resp = requests.get(
                f"{self.base_url}/api/webchat/v2/threads/{thread_id}",
                headers=self.headers,
                timeout=5,
            )
            if resp.status_code == 200:
                data = resp.json()
                return data.get("thread", {}).get("status", "unknown")
        except requests.RequestException:
            pass
        return None

    def has_assistant_message(self, thread_id: str) -> bool:
        """Check if the thread has received an assistant message."""
        try:
            resp = requests.get(
                f"{self.base_url}/api/webchat/v2/threads/{thread_id}/messages",
                headers=self.headers,
                timeout=5,
            )
            if resp.status_code == 200:
                data = resp.json()
                messages = data.get("messages", [])
                return any(m.get("role") == "assistant" for m in messages)
        except requests.RequestException:
            pass
        return False

    def execute_task(
        self,
        task: AgentTask,
        timeout_seconds: int = 120,
        poll_interval: int = 3,
    ) -> AgentResult:
        """Execute a full task: create thread → send message → wait for completion."""
        t_start = time.time()
        task_id = str(uuid.uuid4())[:8]

        # 1. Create thread
        thread_id = self.create_thread()
        if not thread_id:
            return AgentResult(
                task_id=task_id, thread_id="", status="failed",
                error="Failed to create thread", duration_seconds=0.0,
            )

        # 2. Send message
        prompt = task.prompt
        if task.requirements:
            reqs = "\n".join(f"- {r}" for r in task.requirements)
            prompt = f"{prompt}\n\nRequirements:\n{reqs}"

        if not self.send_message(thread_id, prompt):
            return AgentResult(
                task_id=task_id, thread_id=thread_id, status="failed",
                error="Failed to send message",
                duration_seconds=time.time() - t_start,
            )

        # 3. Wait for agent response (poll messages or status)
        deadline = time.time() + timeout_seconds
        while time.time() < deadline:
            # Check if assistant has responded (works with async 202)
            if self.has_assistant_message(thread_id):
                return AgentResult(
                    task_id=task_id, thread_id=thread_id, status="completed",
                    duration_seconds=time.time() - t_start,
                )

            # Also check explicit thread status
            status = self.get_thread_status(thread_id)
            if status in ("completed", "failed", "cancelled"):
                return AgentResult(
                    task_id=task_id, thread_id=thread_id, status=status,
                    duration_seconds=time.time() - t_start,
                )
            time.sleep(poll_interval)

        # Timeout
        return AgentResult(
            task_id=task_id, thread_id=thread_id, status="timeout",
            duration_seconds=time.time() - t_start,
            error=f"Task did not complete within {timeout_seconds}s",
        )
