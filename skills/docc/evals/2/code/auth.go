package auth

import "time"

// docc:begin security-policy#^qd87@hc76 ~rars
const maxFailedAttempts = 5

const lockoutDuration = 15 * time.Minute

// docc:end

// docc:begin security-policy#^sgtk@mrnp ~x53r
const minPasswordLength = 10

// docc:end
