package auth

import "time"

// docc:begin security-policy#^SP-1@hc76 ~rars
const maxFailedAttempts = 5

const lockoutDuration = 15 * time.Minute

// docc:end

// docc:begin security-policy#^SP-2@mrnp ~x53r
const minPasswordLength = 10

// docc:end
