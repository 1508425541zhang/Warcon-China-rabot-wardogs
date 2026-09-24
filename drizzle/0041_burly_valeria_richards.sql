CREATE TABLE "integrity_report_events" (
	"report_id" bigint NOT NULL,
	"instance_id" text NOT NULL,
	"event_id" text NOT NULL,
	"received_at" timestamp with time zone NOT NULL,
	"event" jsonb NOT NULL,
	CONSTRAINT "integrity_report_events_report_id_instance_id_event_id_pk" PRIMARY KEY("report_id","instance_id","event_id")
);
