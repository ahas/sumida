#!/usr/bin/env python3
"""Export/import board system tables to/from JSON.

Usage examples:
  python scripts/bps/board_data_transfer.py --export boards.json
  python scripts/bps/board_data_transfer.py --import boards.json --truncate

The script reads the database URL in this order:
1) --database-url CLI flag
2) DATABASE_URL environment variable
3) SUMIDA_SETTINGS (or settings.toml) file's [database] url field
Install either psycopg (v3) or psycopg2 to run it.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import decimal
import json
import os
import pathlib
import sys
import uuid
from typing import Any, Dict, Iterable, List, Optional, Sequence

try:  # Python 3.11+
  import tomllib
except ImportError:  # pragma: no cover - fallback for Python <=3.10 if tomli is available
  try:
    import tomli as tomllib  # type: ignore
  except ImportError:
    tomllib = None  # type: ignore

try:
  import psycopg
except ImportError:
  psycopg = None  # type: ignore

try:
  import psycopg2
except ImportError:
  psycopg2 = None  # type: ignore


BOARD_TABLES: Sequence[str] = (
  "board_groups",
  "boards",
  "board_topics",
  "board_posts",
  "board_post_reactions",
  "board_post_views",
  "board_post_reports",
  "board_files",
)

TABLE_PRIMARY_KEYS: Dict[str, Optional[str]] = {
  "board_groups": "id",
  "boards": "id",
  "board_topics": "id",
  "board_posts": "id",
  "board_post_reactions": "id",
  "board_post_views": "id",
  "board_post_reports": "id",
  "board_files": None,
}

# Columns that should be sent as json/jsonb, so we wrap them explicitly to avoid
# psycopg2 treating Python lists as arrays.
JSON_COLUMNS: Dict[str, Sequence[str]] = {
  "boards": ("allowed_file_types",),
  "board_posts": ("reactions",),
}


def main() -> None:
  args = parse_args()
  db_url = resolve_database_url(args.database_url, args.settings)
  if not db_url:
    sys.exit("Database URL not found. Provide --database-url, set DATABASE_URL, or configure settings.toml.")

  with connect(db_url) as conn:
    if args.export_path:
      export_tables(conn, args.export_path)
      print(f"Exported board data to {args.export_path}")
    if args.import_path:
      import_tables(conn, args.import_path, truncate_before_load=args.truncate)
      print(f"Imported board data from {args.import_path}")


def parse_args() -> argparse.Namespace:
  parser = argparse.ArgumentParser(description="Export/import board_* tables as JSON.")
  group = parser.add_mutually_exclusive_group(required=True)
  group.add_argument("--export", dest="export_path", help="Export board data to the given JSON file.")
  group.add_argument("--import", dest="import_path", help="Import board data from the given JSON file.")
  parser.add_argument("--database-url", help="Database URL (overrides env/settings).")
  parser.add_argument(
    "--settings",
    help="Path to settings.toml (defaults to SUMIDA_SETTINGS or ./settings.toml if present).",
  )
  parser.add_argument(
    "--truncate",
    action="store_true",
    help="When importing, truncate board tables before loading data.",
  )
  return parser.parse_args()


def resolve_database_url(override_url: Optional[str], settings_path: Optional[str]) -> Optional[str]:
  if override_url:
    return override_url
  env_url = os.getenv("DATABASE_URL")
  if env_url:
    return env_url

  settings_file = (
    settings_path
    or os.getenv("SUMIDA_SETTINGS")
    or os.path.join(os.getcwd(), "settings.toml")
  )
  settings = load_settings(settings_file)
  if settings:
    database_cfg = settings.get("database") or {}
    url = database_cfg.get("url")
    if url:
      return url
  return None


def load_settings(path: str) -> Optional[Dict[str, Any]]:
  candidate = pathlib.Path(path)
  if not candidate.exists():
    return None
  if not tomllib:
    sys.exit("tomllib/tomli is required to parse settings.toml (Python 3.11+ includes tomllib).")
  with candidate.open("rb") as fh:
    return tomllib.load(fh)


def connect(dsn: str):
  if psycopg:
    conn = psycopg.connect(dsn)
    conn.autocommit = False
    return conn
  if psycopg2:
    conn = psycopg2.connect(dsn)
    conn.autocommit = False
    return conn
  sys.exit("Install psycopg (pip install psycopg[binary]) or psycopg2 to use this script.")


def export_tables(conn, output_path: str) -> None:
  data: Dict[str, List[Dict[str, Any]]] = {}
  with conn.cursor() as cur:
    for table in BOARD_TABLES:
      pk = TABLE_PRIMARY_KEYS.get(table)
      order_clause = f"ORDER BY {pk}" if pk else ""
      cur.execute(f"SELECT * FROM {table} {order_clause}")
      rows = fetch_as_dicts(cur)
      data[table] = rows
  serialized = json.dumps(data, default=json_serializer, indent=2, ensure_ascii=False)
  pathlib.Path(output_path).write_text(serialized, encoding="utf-8")


def import_tables(conn, input_path: str, truncate_before_load: bool) -> None:
  payload = json.loads(pathlib.Path(input_path).read_text(encoding="utf-8"))
  missing_tables = [t for t in BOARD_TABLES if t not in payload]
  if missing_tables:
    raise SystemExit(f"Input JSON is missing tables: {', '.join(missing_tables)}")

  cur = conn.cursor()
  try:
    if truncate_before_load:
      truncate_board_tables(cur)
    for table in BOARD_TABLES:
      rows = payload.get(table) or []
      if rows:
        insert_rows(cur, table, rows)
    refresh_sequences(cur)
    conn.commit()
  except Exception:
    conn.rollback()
    raise
  finally:
    cur.close()


def truncate_board_tables(cur) -> None:
  # Reverse order to drop dependent rows first.
  for table in reversed(BOARD_TABLES):
    cur.execute(f"TRUNCATE TABLE {table} RESTART IDENTITY")


def insert_rows(cur, table: str, rows: Iterable[Dict[str, Any]]) -> None:
  rows = list(rows)
  if not rows:
    return
  columns = list(rows[0].keys())
  quoted_columns = [f'"{col}"' for col in columns]
  placeholders = ", ".join([f"%({col})s" for col in columns])
  column_list = ", ".join(quoted_columns)
  sql = f"INSERT INTO {table} ({column_list}) VALUES ({placeholders})"
  for row in rows:
    payload = {col: wrap_json(table, col, val) for col, val in row.items()}
    cur.execute(sql, payload)


def wrap_json(table: str, column: str, value: Any) -> Any:
  if table in JSON_COLUMNS and column in JSON_COLUMNS[table]:
    if psycopg:
      from psycopg.types.json import Json  # type: ignore

      return Json(value)
    if psycopg2:
      from psycopg2.extras import Json  # type: ignore

      return Json(value)
  return value


def refresh_sequences(cur) -> None:
  for table, pk in TABLE_PRIMARY_KEYS.items():
    if not pk:
      continue
    cur.execute(f"SELECT MAX({pk}) FROM {table}")
    max_id = cur.fetchone()[0]
    if max_id is None:
      cur.execute(
        f"SELECT setval(pg_get_serial_sequence(%s, %s), 1, false)",
        (table, pk),
      )
    else:
      cur.execute(
        f"SELECT setval(pg_get_serial_sequence(%s, %s), %s, true)",
        (table, pk, max_id),
      )


def fetch_as_dicts(cur) -> List[Dict[str, Any]]:
  description = cur.description or []
  columns = [getattr(col, "name", col[0]) for col in description]
  records = []
  for row in cur.fetchall():
    # Some cursor implementations return dict-like rows; normalize to dict explicitly.
    if isinstance(row, dict):
      records.append(dict(row))
    else:
      records.append(dict(zip(columns, row)))
  return records


def json_serializer(obj: Any) -> Any:
  if isinstance(obj, (_dt.datetime, _dt.date, _dt.time)):
    return obj.isoformat()
  if isinstance(obj, decimal.Decimal):
    return str(obj)
  if isinstance(obj, uuid.UUID):
    return str(obj)
  raise TypeError(f"Object of type {type(obj).__name__} is not JSON serializable")


if __name__ == "__main__":
  main()
