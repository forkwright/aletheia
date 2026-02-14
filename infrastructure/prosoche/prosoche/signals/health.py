# Health signal — systemd services, docker containers, disk usage
from __future__ import annotations

import asyncio

from loguru import logger

from . import Signal


async def collect(config: dict) -> list[Signal]:
    health_config = config.get("signals", {}).get("health", {})
    if not health_config.get("enabled"):
        return []

    signals = []

    for service in health_config.get("services", []):
        status = await _check_service(service)
        if status != "active":
            signals.append(Signal(
                source="health",
                summary=f"Service {service} is {status}",
                urgency=0.95 if status == "failed" else 0.7,
                relevant_nous=["syn"],
                details=f"systemctl status {service}: {status}",
            ))

    for container in health_config.get("docker_containers", []):
        healthy = await _check_container(container)
        if not healthy:
            signals.append(Signal(
                source="health",
                summary=f"Container {container} is down",
                urgency=0.85,
                relevant_nous=["syn"],
                details=f"docker inspect {container}: not running or unhealthy",
            ))

    disk_warn = health_config.get("disk_warn_pct", 85)
    disk_critical = health_config.get("disk_critical_pct", 95)
    disk_usage = await _check_disk()
    for mount, pct in disk_usage.items():
        if pct >= disk_critical:
            signals.append(Signal(
                source="health",
                summary=f"CRITICAL: {mount} at {pct}% disk usage",
                urgency=1.0,
                relevant_nous=["syn"],
                details=f"Disk {mount}: {pct}% used",
            ))
        elif pct >= disk_warn:
            signals.append(Signal(
                source="health",
                summary=f"Disk warning: {mount} at {pct}%",
                urgency=0.5,
                relevant_nous=["syn"],
                details=f"Disk {mount}: {pct}% used",
            ))

    return signals


async def _check_service(name: str) -> str:
    try:
        proc = await asyncio.create_subprocess_exec(
            "systemctl", "is-active", name,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, _ = await proc.communicate()
        return stdout.decode().strip()
    except Exception as e:
        logger.warning(f"Service check failed for {name}: {e}")
        return "unknown"


async def _check_container(name: str) -> bool:
    try:
        proc = await asyncio.create_subprocess_exec(
            "docker", "inspect", "-f", "{{.State.Running}}", name,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, _ = await proc.communicate()
        return stdout.decode().strip().lower() == "true"
    except Exception:
        return False


async def _check_disk() -> dict[str, int]:
    try:
        proc = await asyncio.create_subprocess_exec(
            "df", "--output=target,pcent", "-x", "tmpfs", "-x", "devtmpfs",
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, _ = await proc.communicate()
        usage = {}
        for line in stdout.decode().strip().split("\n")[1:]:
            parts = line.split()
            if len(parts) >= 2:
                mount = parts[0]
                pct = int(parts[1].rstrip("%"))
                if mount in ("/", "/mnt/ssd", "/mnt/nas"):
                    usage[mount] = pct
        return usage
    except Exception as e:
        logger.warning(f"Disk check failed: {e}")
        return {}
