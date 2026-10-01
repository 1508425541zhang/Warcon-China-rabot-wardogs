-- Better Auth did not persist its randomly generated WebAuthn user handle.
-- Bind legacy handles only after a valid assertion; new Rust registrations store it immediately.
ALTER TABLE passkey ADD COLUMN rust_user_handle text;
