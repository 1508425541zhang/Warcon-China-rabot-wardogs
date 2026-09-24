CREATE TABLE "integrity_weapon_map" (
	"org_id" text NOT NULL,
	"cause" text NOT NULL,
	"category" text NOT NULL,
	"updated_by" text,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "integrity_weapon_map_org_id_cause_pk" PRIMARY KEY("org_id","cause")
);
--> statement-breakpoint
ALTER TABLE "integrity_weapon_map" ADD CONSTRAINT "integrity_weapon_map_org_id_organizations_id_fk" FOREIGN KEY ("org_id") REFERENCES "public"."organizations"("id") ON DELETE cascade ON UPDATE no action;