package auth

import "time"

// docc:begin security-policy#^lockout
const maxFailedAttempts = 5

const lockoutDuration = 15 * time.Minute

// docc:end

// docc:begin security-policy#^password-length
const minPasswordLength = 10

// docc:end
