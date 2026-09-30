import { sql } from 'drizzle-orm';
/** Keep explicit vigilance/kick candidates and all cases linked to punishment history. */
export const retainedCase = sql`(coalesce(integrity_cases.statistical->>'level' IN ('WATCH','KICK_CANDIDATE'),false) OR EXISTS (
 SELECT 1 FROM integrity_actions a WHERE a.case_id=integrity_cases.id AND a.action IN ('KICK','QUARANTINE_24H','QUARANTINE_7D')
))`;
