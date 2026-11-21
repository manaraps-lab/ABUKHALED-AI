#!/usr/bin/env python3
"""Schema/transaction tests; not a substitute for Rust adapter tests."""
from pathlib import Path
import sqlite3
import unittest
ROOT = Path(__file__).resolve().parents[1]
MIGRATION = ROOT / "migrations" / "0001_executor_store.sql"
class ExecutorSchemaTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.execute("PRAGMA foreign_keys=ON")
        self.db.executescript(MIGRATION.read_text(encoding="utf-8"))
    def tearDown(self): self.db.close()
    def insert_task(self, state="Pending", task_id="task-1", lease=None):
        token, deadline, generation = lease or (None, None, None)
        self.db.execute("""INSERT INTO io_tasks(task_id,path,volume_guid,state,source_size_bytes,source_modified_time_ns,source_file_id,attempt,lease_token,lease_deadline_ms,lease_topology_generation,last_failure) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)""",
        (task_id, r"C:\data\a.bin", r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}", state, "18446744073709551615", "170000000000000000000", None, 0, token, deadline, generation, None))
    def test_migration_and_initial_token(self):
        self.assertEqual(self.db.execute("SELECT value FROM app_meta WHERE key='executor_next_token'").fetchone()[0], "1")
        self.assertEqual(self.db.execute("SELECT version FROM schema_migrations").fetchone()[0], 1)
    def test_full_u64_and_i128_values_as_text(self):
        self.insert_task()
        self.assertEqual(self.db.execute("SELECT source_size_bytes,source_modified_time_ns FROM io_tasks").fetchone(), ("18446744073709551615", "170000000000000000000"))
    def test_leased_state_requires_all_fencing_fields(self):
        with self.assertRaises(sqlite3.IntegrityError): self.insert_task(state="Leased")
        self.insert_task(state="Leased", lease=("7", "900", "3"))
    def test_non_leased_state_rejects_stale_lease(self):
        with self.assertRaises(sqlite3.IntegrityError): self.insert_task(lease=("7", "900", "3"))
    def test_checkpoint_fk_and_cascade(self):
        with self.assertRaises(sqlite3.IntegrityError): self.db.execute("INSERT INTO io_checkpoints VALUES(?,?,?,?,?,?)", ("missing", "1", b"x", "1", "1", None))
        self.insert_task()
        self.db.execute("INSERT INTO io_checkpoints VALUES(?,?,?,?,?,?)", ("task-1", "5", b"processor-v1", "18446744073709551615", "170000000000000000000", None))
        self.db.execute("DELETE FROM io_tasks WHERE task_id='task-1'")
        self.assertEqual(self.db.execute("SELECT count(*) FROM io_checkpoints").fetchone()[0], 0)
    def test_invalid_state_and_failure_rejected(self):
        with self.assertRaises(sqlite3.IntegrityError): self.insert_task(state="Skipped")
        self.insert_task()
        with self.assertRaises(sqlite3.IntegrityError): self.db.execute("UPDATE io_tasks SET last_failure='UnknownFailure' WHERE task_id='task-1'")
    def test_snapshot_transaction_rolls_back_all_rows_and_token(self):
        try:
            self.db.execute("BEGIN IMMEDIATE")
            self.insert_task()
            self.db.execute("INSERT INTO io_checkpoints VALUES(?,?,?,?,?,?)", ("task-1", "3", b"state", "10", "100", None))
            self.db.execute("UPDATE app_meta SET value='9' WHERE key='executor_next_token'")
            self.db.execute("INSERT INTO io_tasks(task_id,path,volume_guid,state,source_size_bytes,source_modified_time_ns,attempt) VALUES('bad','','v','Bogus','1','1',0)")
            self.db.commit()
        except sqlite3.IntegrityError: self.db.rollback()
        self.assertEqual(self.db.execute("SELECT count(*) FROM io_tasks").fetchone()[0], 0)
        self.assertEqual(self.db.execute("SELECT value FROM app_meta WHERE key='executor_next_token'").fetchone()[0], "1")
    def test_migration_is_idempotent(self):
        self.db.executescript(MIGRATION.read_text(encoding="utf-8"))
        self.assertEqual(self.db.execute("SELECT count(*) FROM schema_migrations WHERE version=1").fetchone()[0], 1)
if __name__ == "__main__": unittest.main(verbosity=2)
