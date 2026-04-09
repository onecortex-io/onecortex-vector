-- Enables geographic distance calculations via the earthdistance extension.
-- cube is earthdistance's required dependency; both ship with postgresql-contrib.
CREATE EXTENSION IF NOT EXISTS cube;
CREATE EXTENSION IF NOT EXISTS earthdistance;
