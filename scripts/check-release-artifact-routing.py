#!/usr/bin/env python3
"""Validate Aletheia's one-owner release-to-artifact workflow contract."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import tomllib
import yaml

RELEASE_PLEASE = Path(".github/workflows/release-please.yml")
RELEASE = Path(".github/workflows/release.yml")
RELEASE_HEALTH = Path(".github/workflows/release-health.yml")
GATE = Path(".github/workflows/gate-attestation.yml")
SECURITY = Path(".github/workflows/security.yml")
CONFIG = Path("release-please-config.json")
DEPLOY = Path("scripts/deploy.sh")
CROSS_CONFIG = Path("Cross.toml")
CROSS_INSTALLER = Path("scripts/install-cargo-auditable-cross.sh")
CONSUMER_DOCS = (
    Path("README.md"),
    Path("docs/QUICKSTART.md"),
    Path("docs/UPGRADING.md"),
    Path("docs/RELEASING.md"),
)
EXACT_RELEASE_REF = "${{ inputs.release_sha || github.sha }}"


class UniqueKeyLoader(yaml.SafeLoader):
    """Safe YAML loader that rejects mappings whose last key would silently win."""


def _construct_unique_mapping(
    loader: UniqueKeyLoader, node: yaml.nodes.MappingNode, deep: bool = False
) -> dict:
    mapping: dict = {}
    for key_node, value_node in node.value:
        key = loader.construct_object(key_node, deep=deep)
        if key in mapping:
            raise yaml.constructor.ConstructorError(
                "while constructing a mapping",
                node.start_mark,
                f"found duplicate key {key!r}",
                key_node.start_mark,
            )
        mapping[key] = loader.construct_object(value_node, deep=deep)
    return mapping


UniqueKeyLoader.add_constructor(
    yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, _construct_unique_mapping
)


def _load_workflow(root: Path, path: Path, errors: list[str]) -> dict:
    try:
        value = yaml.load(
            (root / path).read_text(encoding="utf-8"), Loader=UniqueKeyLoader
        )
    except (OSError, yaml.YAMLError) as exc:
        errors.append(f"{path}: failed to load: {exc}")
        return {}
    if not isinstance(value, dict):
        errors.append(f"{path}: workflow root must be a mapping")
        return {}
    return value


def _triggers(workflow: dict) -> dict:
    value = workflow.get("on", workflow.get(True, {}))
    return value if isinstance(value, dict) else {}


def _step_text(job: dict) -> str:
    chunks: list[str] = []
    for step in job.get("steps", []):
        if not isinstance(step, dict):
            continue
        chunks.extend(
            str(step.get(key, "")) for key in ("name", "uses", "if", "run", "with")
        )
    return "\n".join(chunks)


def _needs(job: dict) -> set[str]:
    value = job.get("needs", [])
    if isinstance(value, str):
        return {value}
    if isinstance(value, list):
        return {item for item in value if isinstance(item, str)}
    return set()


def _find_step(job: dict, name: str) -> dict | None:
    for step in job.get("steps", []):
        if isinstance(step, dict) and step.get("name") == name:
            return step
    return None


def _check_release_please(workflow: dict) -> list[str]:
    errors: list[str] = []
    jobs = workflow.get("jobs", {})
    control = jobs.get("release-please", {}) if isinstance(jobs, dict) else {}
    triggers = _triggers(workflow)
    push = triggers.get("push", {})
    branches = push.get("branches", []) if isinstance(push, dict) else []
    if branches != ["main"] or "workflow_dispatch" in triggers:
        errors.append(
            f"{RELEASE_PLEASE}: release mutation must be triggered only by main push"
        )

    concurrency = control.get("concurrency", {})
    if (
        not isinstance(concurrency, dict)
        or concurrency.get("cancel-in-progress") is not False
    ):
        errors.append(
            f"{RELEASE_PLEASE}: release-please control job must queue, not cancel"
        )
    if not isinstance(concurrency, dict) or concurrency.get("queue") != "max":
        errors.append(
            f"{RELEASE_PLEASE}: release-please control job must retain every pending run"
        )
    if (
        not isinstance(concurrency, dict)
        or concurrency.get("group") != "release-please-control-${{ github.ref }}"
    ):
        errors.append(
            f"{RELEASE_PLEASE}: release-please control must serialize the main lane"
        )
    if "concurrency" in workflow:
        errors.append(
            f"{RELEASE_PLEASE}: workflow-level concurrency can cancel artifact handoff"
        )

    permissions = control.get("permissions", {})
    if not isinstance(permissions, dict) or permissions.get("actions") != "write":
        errors.append(f"{RELEASE_PLEASE}: tagged dispatch lacks actions: write")

    release_action: dict | None = None
    for step in control.get("steps", []):
        if isinstance(step, dict) and str(step.get("uses", "")).startswith(
            "googleapis/release-please-action@"
        ):
            release_action = step
            break
    if release_action is None:
        errors.append(f"{RELEASE_PLEASE}: release-please action is missing")
    elif "token" in (release_action.get("with", {}) or {}):
        errors.append(
            f"{RELEASE_PLEASE}: custom Release Please token would duplicate tag routing"
        )

    dispatch = _find_step(control, "Dispatch artifacts for the exact released tag")
    if dispatch is None:
        errors.append(f"{RELEASE_PLEASE}: release_created has no tagged artifact dispatch")
    else:
        dispatch_text = _step_text({"steps": [dispatch]})
        if dispatch.get("if") != "steps.release.outputs.release_created == 'true'":
            errors.append(f"{RELEASE_PLEASE}: artifact dispatch is not gated by release_created")
        dispatch_env = dispatch.get("env", {})
        expected_env = {
            "GH_REPO": "${{ github.repository }}",
            "GH_TOKEN": "${{ secrets.GITHUB_TOKEN }}",
            "RELEASE_TAG": "${{ steps.release.outputs.tag_name }}",
            "RELEASE_SHA": "${{ steps.release.outputs.sha }}",
        }
        for key, expected in expected_env.items():
            if not isinstance(dispatch_env, dict) or dispatch_env.get(key) != expected:
                errors.append(f"{RELEASE_PLEASE}: dispatch env {key} is not exact")
        for required in (
            "(-[0-9A-Za-z.-]+)?(\\+[0-9A-Za-z.-]+)?",
            "^[0-9a-f]{40}$",
            "git ls-remote --tags origin",
            '"refs/tags/${RELEASE_TAG}"',
            '"refs/tags/${RELEASE_TAG}^{}"',
            'live_tag_commit="${peeled:-$direct}"',
            'if [[ "$live_tag_commit" != "$RELEASE_SHA" ]]',
            "gh workflow run release.yml",
            '--ref "$RELEASE_TAG"',
            '--field tag_name="$RELEASE_TAG"',
            '--field release_sha="$RELEASE_SHA"',
        ):
            if required not in dispatch_text:
                errors.append(f"{RELEASE_PLEASE}: tagged dispatch lacks {required}")
    for job_name, job in (jobs.items() if isinstance(jobs, dict) else ()):
        if job_name != "release-please" and isinstance(job, dict) and "uses" in job:
            errors.append(
                f"{RELEASE_PLEASE}: {job_name} bypasses the durable tagged dispatch"
            )
    return errors


def _check_release(workflow: dict) -> list[str]:
    errors: list[str] = []
    triggers = _triggers(workflow)
    push = triggers.get("push", {})
    tags = push.get("tags", []) if isinstance(push, dict) else []
    if "v*" not in tags:
        errors.append(f"{RELEASE}: manual v* tag fallback is missing")
    if "workflow_call" in triggers:
        errors.append(f"{RELEASE}: reusable-call route must not bypass the tagged event")
    dispatch = triggers.get("workflow_dispatch", {})
    inputs = dispatch.get("inputs", {}) if isinstance(dispatch, dict) else {}
    for name in ("tag_name", "release_sha"):
        spec = inputs.get(name, {}) if isinstance(inputs, dict) else {}
        if not isinstance(spec, dict) or spec.get("required") is not True:
            errors.append(f"{RELEASE}: workflow_dispatch input {name} must be required")

    env = workflow.get("env", {})
    if not isinstance(env, dict) or env.get("GH_REPO") != "${{ github.repository }}":
        errors.append(f"{RELEASE}: GitHub CLI calls lack an explicit repository owner")

    jobs = workflow.get("jobs", {})
    if not isinstance(jobs, dict):
        return [f"{RELEASE}: jobs must be a mapping"]
    identity = jobs.get("release-identity", {})
    identity_text = _step_text(identity)
    for required in (
        "check-release-versioning.py verify-release",
        "git rev-list -n 1",
        "RELEASE_SHA",
        "CALLER_SHA",
        "CALLER_REF",
        'refs/tags/${RELEASE_TAG}',
    ):
        if required not in identity_text:
            errors.append(f"{RELEASE}: release identity preflight lacks {required}")
    for job_name in ("canonical-gate", "canonical-security"):
        if "release-identity" not in _needs(jobs.get(job_name, {})):
            errors.append(f"{RELEASE}: {job_name} must wait for release identity")
    gate_call = jobs.get("canonical-gate", {})
    if gate_call.get("uses") != "./.github/workflows/gate-attestation.yml":
        errors.append(f"{RELEASE}: canonical-gate does not call the gate owner")
    if (gate_call.get("with", {}) or {}).get("release_mode") is not True:
        errors.append(f"{RELEASE}: canonical-gate does not enable release mode")
    security_call = jobs.get("canonical-security", {})
    if security_call.get("uses") != "./.github/workflows/security.yml":
        errors.append(f"{RELEASE}: canonical-security does not call the security owner")
    if (security_call.get("with", {}) or {}).get("release_sha") != EXACT_RELEASE_REF:
        errors.append(f"{RELEASE}: canonical-security is not bound to release_sha")

    prepare_text = _step_text(jobs.get("prepare-release", {}))
    for required in (
        "expected_prerelease",
        "isPrerelease",
        '--prerelease="$expected_prerelease"',
    ):
        if required not in prepare_text:
            errors.append(f"{RELEASE}: draft prerelease identity lacks {required}")

    checkout_count = 0
    for job_name, job in jobs.items():
        if not isinstance(job, dict):
            continue
        for step in job.get("steps", []):
            if not isinstance(step, dict) or not str(step.get("uses", "")).startswith(
                "actions/checkout@"
            ):
                continue
            checkout_count += 1
            with_block = step.get("with", {})
            if not isinstance(with_block, dict) or with_block.get("ref") != EXACT_RELEASE_REF:
                errors.append(
                    f"{RELEASE}: {job_name} checkout is not bound to release_sha"
                )
    if checkout_count == 0:
        errors.append(f"{RELEASE}: no exact-source checkouts found")

    publish = jobs.get("publish-release", {})
    if not {"build", "sbom"}.issubset(_needs(publish)):
        errors.append(f"{RELEASE}: final publication must wait for build and SBOM jobs")
    steps = publish.get("steps", []) if isinstance(publish, dict) else []
    names = [step.get("name") for step in steps if isinstance(step, dict)]
    required_names = (
        "Validate the complete staged contract",
        "Upload the complete set to the draft",
        "Round-trip the draft assets before publication",
        "Publish the verified draft",
    )
    if not all(name in names for name in required_names):
        errors.append(f"{RELEASE}: final validation/upload/publication barrier is incomplete")
    elif [names.index(name) for name in required_names] != sorted(
        names.index(name) for name in required_names
    ):
        errors.append(f"{RELEASE}: final release barrier steps are out of order")
    if not names or names[-1] != "Publish the verified draft":
        errors.append(f"{RELEASE}: publication must be the final step")

    upload_step = _find_step(publish, "Upload the complete set to the draft")
    upload_text = _step_text({"steps": [upload_step]}) if upload_step else ""
    upload_at = upload_text.find("gh release upload")
    for guard in (
        "jq -r '.isDraft'",
        "git ls-remote --tags origin",
        "live_tag_commit",
    ):
        guard_at = upload_text.find(guard)
        if guard_at < 0 or upload_at < 0 or guard_at > upload_at:
            errors.append(
                f"{RELEASE}: draft upload lacks a pre-mutation {guard} guard"
            )

    publish_text = _step_text(publish)
    for required in (
        "check-release-assets.py",
        "check-release-tarball.sh",
        "check-release-attestations.py",
        "gh release download",
        "git ls-remote --tags origin",
        '"refs/tags/${RELEASE_TAG}^{}"',
        '"$live_tag_commit" != "$RELEASE_SHA"',
        "gh release edit",
    ):
        if required not in publish_text:
            errors.append(f"{RELEASE}: final publication barrier lacks {required}")
    for job_name, job in jobs.items():
        if job_name == "publish-release" or not isinstance(job, dict):
            continue
        if "gh release upload" in _step_text(job):
            errors.append(f"{RELEASE}: {job_name} mutates release assets before barrier")
    if publish_text.count("gh release edit") != 1:
        errors.append(f"{RELEASE}: release publication must have exactly one owner")

    outcome = jobs.get("release-outcome", {})
    expected_outcome_needs = {
        "release-identity", "canonical-gate", "canonical-security", "prepare-release",
        "test", "feature-policy", "feature-check", "no-default-recipes", "build", "sbom",
        "publish-release",
    }
    if not expected_outcome_needs.issubset(_needs(outcome)):
        errors.append(f"{RELEASE}: release outcome is not terminal")
    if outcome.get("if") != "${{ always() }}":
        errors.append(f"{RELEASE}: release outcome must run on every terminal state")
    outcome_permissions = outcome.get("permissions", {})
    if outcome_permissions != {"actions": "read", "contents": "read"}:
        errors.append(f"{RELEASE}: release outcome permissions are not read-only")
    outcome_text = _step_text(outcome)
    for required in (
        "scripts/check-release-outcome.py",
        "--attempts 6 --retry-seconds 10",
    ):
        if required not in outcome_text:
            errors.append(f"{RELEASE}: release outcome lacks {required}")
    outcome_step = _find_step(outcome, "Report the release outcome")
    outcome_env = outcome_step.get("env", {}) if isinstance(outcome_step, dict) else {}
    if not isinstance(outcome_env, dict) or outcome_env.get("RUN_ID") != "${{ github.run_id }}":
        errors.append(f"{RELEASE}: release outcome run identity is not exact")

    build_text = _step_text(jobs.get("build", {}))
    for required in (
        "cargo auditable build --locked",
        "cross build --locked",
        "check-auditable-info.py",
        '--cyclonedx "$BINARY.cdx.json"',
        '--spdx "$BINARY.spdx.json"',
    ):
        if required not in build_text:
            errors.append(f"{RELEASE}: artifact build lacks {required}")
    if "cross auditable" in build_text:
        errors.append(f"{RELEASE}: cross auditable runs on the host instead of the image")
    decode = _find_step(
        jobs.get("build", {}), "Verify embedded auditable dependency graph"
    )
    decode_run = str(decode.get("run", "")) if isinstance(decode, dict) else ""
    for required in (
        'audit_dir=$(mktemp -d "$GITHUB_WORKSPACE/.release-audit.XXXXXX")',
        'audit_info="$audit_dir/auditable-info.json"',
        'rust-audit-info "$binary" > "$audit_info"',
        "printf 'AUDITABLE_INFO=%s\\n' \"$audit_info\" >> \"$GITHUB_ENV\"",
        '"$audit_info" "$VERSION"',
    ):
        if required not in decode_run:
            errors.append(f"{RELEASE}: auditable evidence handoff is not exact")
            break
    for step_name in ("Generate CycloneDX SBOM", "Generate SPDX SBOM"):
        step = _find_step(jobs.get("build", {}), step_name)
        step_with = step.get("with", {}) if isinstance(step, dict) else {}
        if (
            not isinstance(step_with, dict)
            or step_with.get("file") != "${{ steps.artifact.outputs.bin }}"
            or "path" in step_with
        ):
            errors.append(
                f"{RELEASE}: {step_name} must scan the binary as a file"
            )
    bind_sbom = _find_step(jobs.get("build", {}), "Bind SBOM package inventories to the binary")
    bind_env = bind_sbom.get("env", {}) if isinstance(bind_sbom, dict) else {}
    bind_run = str(bind_sbom.get("run", "")) if isinstance(bind_sbom, dict) else ""
    if (
        not isinstance(bind_env, dict)
        or bind_env != {"VERSION": "${{ steps.version.outputs.version }}"}
        or '"$AUDITABLE_INFO"' not in bind_run
    ):
        errors.append(
            f"{RELEASE}: binary SBOM comparison lacks the exact evidence handoff"
        )
    return errors


def _check_release_health(workflow: dict) -> list[str]:
    errors: list[str] = []
    triggers = _triggers(workflow)
    schedule = triggers.get("schedule")
    if not isinstance(schedule, list) or not any(
        isinstance(entry, dict) and entry.get("cron") == "43 6 * * *" for entry in schedule
    ):
        errors.append(f"{RELEASE_HEALTH}: daily scheduled reconciliation is missing")
    if "workflow_dispatch" not in triggers:
        errors.append(f"{RELEASE_HEALTH}: manual reconciliation is missing")
    if workflow.get("permissions") != {"contents": "read"}:
        errors.append(f"{RELEASE_HEALTH}: audit permissions are not minimal read-only")
    jobs = workflow.get("jobs", {})
    audit = jobs.get("audit", {}) if isinstance(jobs, dict) else {}
    if audit.get("timeout-minutes") != 10:
        errors.append(f"{RELEASE_HEALTH}: reconciliation is not bounded")
    audit_text = _step_text(audit)
    for required in (
        "scripts/check-release-health.py --grace-hours 12",
    ):
        if required not in audit_text:
            errors.append(f"{RELEASE_HEALTH}: audit lacks {required}")
    audit_step = _find_step(audit, "Reconcile tags against releases")
    audit_env = audit_step.get("env", {}) if isinstance(audit_step, dict) else {}
    expected_env = {"GH_TOKEN": "${{ secrets.GITHUB_TOKEN }}", "GH_REPO": "${{ github.repository }}"}
    if not isinstance(audit_env, dict) or audit_env != expected_env:
        errors.append(f"{RELEASE_HEALTH}: audit identity environment is not exact")
    for forbidden in ("gh release create", "gh release edit", "gh release upload", "gh issue create"):
        if forbidden in audit_text:
            errors.append(f"{RELEASE_HEALTH}: read-only audit contains {forbidden}")
    return errors


def _check_supporting_workflows(gate: dict, security: dict) -> list[str]:
    errors: list[str] = []
    gate_call = _triggers(gate).get("workflow_call", {})
    gate_inputs = gate_call.get("inputs", {}) if isinstance(gate_call, dict) else {}
    if "release_mode" not in gate_inputs:
        errors.append(f"{GATE}: release_mode workflow_call input is missing")
    gate_jobs = gate.get("jobs", {})
    hybrid = gate_jobs.get("hybrid-gate", {}) if isinstance(gate_jobs, dict) else {}
    if (hybrid.get("with", {}) or {}).get("docs_only_exemption") != "${{ !inputs.release_mode }}":
        errors.append(f"{GATE}: release mode does not disable docs-only exemption")
    if "-caller-" not in str((gate.get("concurrency", {}) or {}).get("group", "")):
        errors.append(f"{GATE}: caller concurrency group is not disjoint")

    security_call = _triggers(security).get("workflow_call", {})
    security_inputs = (
        security_call.get("inputs", {}) if isinstance(security_call, dict) else {}
    )
    release_input = security_inputs.get("release_sha", {})
    if (
        not isinstance(release_input, dict)
        or release_input.get("required") is not False
        or release_input.get("default") != ""
        or release_input.get("type") != "string"
    ):
        errors.append(f"{SECURITY}: release_sha workflow_call input is missing")
    if "-security-" not in str((security.get("concurrency", {}) or {}).get("group", "")):
        errors.append(f"{SECURITY}: nested security concurrency group is not disjoint")
    security_jobs = security.get("jobs", {}) or {}
    for job_name, job in security_jobs.items():
        if not isinstance(job, dict):
            continue
        for step in job.get("steps", []):
            if isinstance(step, dict) and str(step.get("uses", "")).startswith(
                "actions/checkout@"
            ):
                with_block = step.get("with", {})
                if not isinstance(with_block, dict) or with_block.get("ref") != EXACT_RELEASE_REF:
                    errors.append(
                        f"{SECURITY}: {job_name} checkout is not bound to release_sha"
                    )
    secret_job = security_jobs.get("secret-scan", {})
    secret_checkout = next(
        (
            step
            for step in secret_job.get("steps", [])
            if isinstance(step, dict)
            and str(step.get("uses", "")).startswith("actions/checkout@")
        ),
        None,
    )
    secret_checkout_with = (
        secret_checkout.get("with", {}) if isinstance(secret_checkout, dict) else {}
    )
    if (
        not isinstance(secret_checkout_with, dict)
        or secret_checkout_with.get("fetch-depth") != 0
    ):
        errors.append(f"{SECURITY}: TruffleHog release scan lacks full history")
    trufflehog = _find_step(secret_job, "TruffleHog secret scan")
    trufflehog_with = trufflehog.get("with", {}) if isinstance(trufflehog, dict) else {}
    if (
        not isinstance(trufflehog_with, dict)
        or trufflehog_with.get("base") != ""
        or trufflehog_with.get("head") != "${{ inputs.release_sha }}"
    ):
        errors.append(f"{SECURITY}: TruffleHog release scan is not bound to release_sha")
    if (
        not isinstance(trufflehog_with, dict)
        or str(trufflehog_with.get("version")) != "3.97.0"
    ):
        errors.append(f"{SECURITY}: TruffleHog runtime is not pinned")

    gitleaks_job = security_jobs.get("gitleaks", {})
    gitleaks_checkout = next(
        (
            step
            for step in gitleaks_job.get("steps", [])
            if isinstance(step, dict)
            and str(step.get("uses", "")).startswith("actions/checkout@")
        ),
        None,
    )
    gitleaks_checkout_with = (
        gitleaks_checkout.get("with", {})
        if isinstance(gitleaks_checkout, dict)
        else {}
    )
    if (
        not isinstance(gitleaks_checkout_with, dict)
        or gitleaks_checkout_with.get("fetch-depth") != 0
    ):
        errors.append(f"{SECURITY}: Gitleaks release scan lacks full history")
    action_step: dict | None = None
    action_index = -1
    for index, step in enumerate(gitleaks_job.get("steps", [])):
        if isinstance(step, dict) and str(step.get("uses", "")).startswith(
            "gitleaks/gitleaks-action@"
        ):
            action_step = step
            action_index = index
            break
    action_env = action_step.get("env", {}) if action_step is not None else {}
    if not isinstance(action_env, dict) or str(action_env.get("GITLEAKS_VERSION")) != "8.24.3":
        errors.append(f"{SECURITY}: Gitleaks runtime is not pinned")
    release_scan = _find_step(gitleaks_job, "Gitleaks release-history scan")
    if (
        release_scan is None
        or release_scan.get("if")
        != "inputs.release_sha != '' && github.event_name == 'push'"
    ):
        errors.append(f"{SECURITY}: Gitleaks release-history scan is missing")
    else:
        release_index = gitleaks_job.get("steps", []).index(release_scan)
        if action_index < 0 or release_index <= action_index:
            errors.append(
                f"{SECURITY}: Gitleaks release-history scan must follow its installer"
            )
        release_env = release_scan.get("env", {})
        if (
            not isinstance(release_env, dict)
            or release_env.get("RELEASE_SHA") != "${{ inputs.release_sha }}"
        ):
            errors.append(f"{SECURITY}: Gitleaks release SHA env is not exact")
        release_text = _step_text({"steps": [release_scan]})
        for required in (
            "git rev-parse HEAD",
            "^[0-9a-f]{40}$",
            "--is-shallow-repository",
            "gitleaks git",
            ".gitleaks.toml",
            "--exit-code 2",
            "--full-history ${RELEASE_SHA}",
        ):
            if required not in release_text:
                errors.append(f"{SECURITY}: release-history scan lacks {required}")
    return errors


def _check_cross_contract(root: Path) -> list[str]:
    errors: list[str] = []
    try:
        with (root / CROSS_CONFIG).open("rb") as handle:
            config = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        return [f"{CROSS_CONFIG}: failed to load: {exc}"]
    pre_build = (config.get("build", {}) or {}).get("pre-build")
    if pre_build != "./scripts/install-cargo-auditable-cross.sh":
        errors.append(
            f"{CROSS_CONFIG}: pre-build must be the scalar copied-script form"
        )
    try:
        installer = (root / CROSS_INSTALLER).read_text(encoding="utf-8")
    except OSError as exc:
        return [*errors, f"{CROSS_INSTALLER}: failed to read: {exc}"]
    for required in (
        'cargo_auditable_version="0.7.4"',
        "4a4f0c124543c065f03d89aee26550305143c6e4af3e46270dbabefeb79895d2",
        "/usr/local/bin/cargo-auditable",
        'exec /rust/bin/cargo auditable "$@"',
        "--proto '=https' --proto-redir '=https'",
    ):
        if required not in installer:
            errors.append(f"{CROSS_INSTALLER}: cross-image install lacks {required}")
    return errors


def _check_consumers(root: Path) -> list[str]:
    errors: list[str] = []
    try:
        deploy = (root / DEPLOY).read_text(encoding="utf-8")
    except OSError as exc:
        return [f"{DEPLOY}: failed to read: {exc}"]
    if '${asset_name}-*' in deploy:
        errors.append(f"{DEPLOY}: release download uses a multi-asset glob")
    for required in (
        '--pattern "$versioned_asset"',
        '--pattern "$checksum_asset"',
        'checksum_url="${url}.sha256"',
        'scripts/verify-sha256.sh',
        "refusing an unrequested source build",
        'gh release view "$version"',
        "--json isDraft",
        "probe_service_state()",
        "service_state=$(probe_service_state)",
        "rollback_service_state=$(probe_service_state)",
        'install -m 0755 -- "$backup" "$rollback_tmp"',
        'smoke_test "$rollback_tmp"',
        "Smoke test failed — production binary unchanged",
    ):
        if required not in deploy:
            errors.append(f"{DEPLOY}: exact verified download lacks {required}")
    if deploy.count("--proto '=https' --proto-redir '=https'") < 2:
        errors.append(f"{DEPLOY}: binary and checksum downloads must remain HTTPS-only")
    if "Download failed; proceeding with local build" in deploy:
        errors.append(f"{DEPLOY}: explicit release download silently source-builds")
    verify_at = deploy.find('scripts/verify-sha256.sh')
    chmod_at = deploy.find("chmod +x", max(verify_at, 0))
    if verify_at < 0 or chmod_at < verify_at:
        errors.append(f"{DEPLOY}: downloaded binary becomes executable before verification")

    for path in CONSUMER_DOCS:
        try:
            text = (root / path).read_text(encoding="utf-8")
        except OSError as exc:
            errors.append(f"{path}: failed to read: {exc}")
            continue
        for required in ('TAG=', 'VERSION="${TAG#v}"'):
            if required not in text:
                errors.append(f"{path}: install contract lacks {required}")
        if path != Path("docs/RELEASING.md"):
            if "/releases/download/${TAG}/" not in text:
                errors.append(f"{path}: release URL does not use the exact tag")
            if "${VERSION}" not in text:
                errors.append(f"{path}: asset/root name does not use the bare version")
        else:
            for required in (
                'gh workflow run release.yml --ref "$TAG"',
                "git push origin main",
                "git push origin refs/tags/",
            ):
                if required not in text:
                    errors.append(f"{path}: release recovery contract lacks {required}")
            if "git push origin main --tags" in text:
                errors.append(f"{path}: manual release pushes an unbounded tag set")
    return errors


def check_repo(root: Path) -> list[str]:
    errors: list[str] = []
    workflows = {
        path: _load_workflow(root, path, errors)
        for path in (RELEASE_PLEASE, RELEASE, RELEASE_HEALTH, GATE, SECURITY)
    }
    try:
        config = json.loads((root / CONFIG).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        errors.append(f"{CONFIG}: failed to load: {exc}")
        config = {}
    if not isinstance(config, dict) or config.get("draft") is not True:
        errors.append(f"{CONFIG}: draft must be true")
    if not isinstance(config, dict) or config.get("force-tag-creation") is not True:
        errors.append(f"{CONFIG}: force-tag-creation must be true")

    errors.extend(_check_release_please(workflows[RELEASE_PLEASE]))
    errors.extend(_check_release(workflows[RELEASE]))
    errors.extend(_check_release_health(workflows[RELEASE_HEALTH]))
    errors.extend(_check_supporting_workflows(workflows[GATE], workflows[SECURITY]))
    errors.extend(_check_cross_contract(root))
    errors.extend(_check_consumers(root))
    return errors


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    errors = check_repo(args.root.resolve())
    if errors:
        for error in errors:
            print(f"release-routing: {error}", file=sys.stderr)
        return 1
    print("release-routing: one exact release-to-artifact path verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
