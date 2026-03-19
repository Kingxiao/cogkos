-- Allow NULL description in conflict_records (defensive, code now generates description)
ALTER TABLE conflict_records ALTER COLUMN description DROP NOT NULL;
