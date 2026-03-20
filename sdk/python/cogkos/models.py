"""CogKOS SDK data models."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class Belief:
    """A single epistemic claim from the knowledge base."""

    content: str
    confidence: float
    node_type: str = ""
    tags: list[str] = field(default_factory=list)
    claim_id: str | None = None
    source: dict[str, Any] | None = None
    activation_weight: float = 0.0

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> Belief:
        return cls(
            content=data.get("content", ""),
            confidence=data.get("confidence", 0.0),
            node_type=data.get("node_type", ""),
            tags=data.get("tags", []),
            claim_id=data.get("claim_id") or data.get("id"),
            source=data.get("source"),
            activation_weight=data.get("activation_weight", 0.0),
        )


@dataclass
class Conflict:
    """A conflict between two beliefs."""

    type: str
    description: str
    claim_ids: list[str] = field(default_factory=list)

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> Conflict:
        return cls(
            type=data.get("type", ""),
            description=data.get("description", ""),
            claim_ids=data.get("claim_ids", []),
        )


@dataclass
class RecallResult:
    """Result from a knowledge query."""

    best_belief: str
    beliefs: list[Belief]
    related: list[Belief]
    conflicts: list[Conflict]
    predictions: list[dict[str, Any]]
    gaps: list[dict[str, Any]]
    query_hash: int
    processing_path: str = ""
    raw: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_response(cls, data: dict[str, Any]) -> RecallResult:
        beliefs_raw = data.get("beliefs", [])
        beliefs = [Belief.from_dict(b) for b in beliefs_raw]
        related_raw = data.get("related", data.get("graph_related", []))
        related = [Belief.from_dict(r) for r in related_raw]
        conflicts_raw = data.get("conflicts", [])
        conflicts = [Conflict.from_dict(c) for c in conflicts_raw]

        best = ""
        if beliefs:
            best = beliefs[0].content
        elif data.get("best_belief"):
            best = data["best_belief"]

        return cls(
            best_belief=best,
            beliefs=beliefs,
            related=related,
            conflicts=conflicts,
            predictions=data.get("predictions", []),
            gaps=data.get("gaps", []),
            query_hash=data.get("query_hash", 0),
            processing_path=data.get("processing_path", ""),
            raw=data,
        )


@dataclass
class LearnResult:
    """Result from submitting knowledge."""

    claim_id: str
    status: str
    conflicts_detected: int = 0
    raw: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_response(cls, data: dict[str, Any]) -> LearnResult:
        return cls(
            claim_id=data.get("claim_id", ""),
            status=data.get("status", "accepted"),
            conflicts_detected=data.get("conflicts_detected", 0),
            raw=data,
        )


@dataclass
class FeedbackResult:
    """Result from submitting feedback."""

    status: str
    claims_updated: int = 0
    raw: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_response(cls, data: dict[str, Any]) -> FeedbackResult:
        return cls(
            status=data.get("status", "accepted"),
            claims_updated=data.get("claims_updated", 0),
            raw=data,
        )


@dataclass
class GapResult:
    """Result from reporting a knowledge gap."""

    gap_id: str
    status: str
    raw: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_response(cls, data: dict[str, Any]) -> GapResult:
        return cls(
            gap_id=data.get("gap_id", ""),
            status=data.get("status", "recorded"),
            raw=data,
        )
