ALTER TABLE "integrity_ai_settings" ADD COLUMN "auto_close_enabled" boolean DEFAULT true NOT NULL;
--> statement-breakpoint
ALTER TABLE "integrity_ai_settings" ADD COLUMN "delete_low_risk" boolean DEFAULT true NOT NULL;
