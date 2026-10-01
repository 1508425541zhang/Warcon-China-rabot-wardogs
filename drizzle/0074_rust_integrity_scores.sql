-- Rule weights can be fractional. Preserve the same value in scores and frozen cases.
ALTER TABLE integrity_scores ALTER COLUMN score TYPE double precision;
ALTER TABLE integrity_cases ALTER COLUMN risk_score TYPE double precision;
