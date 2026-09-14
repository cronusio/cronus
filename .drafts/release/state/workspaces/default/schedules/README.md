# Schedules (per office)

Advanced-alarm-clock scheduling. Each file = one schedule.

Kinds: recurring | one-shot (one-shot auto-deletes after firing).
Recurrence (friendly): weekdays | weekends | daily | days | interval + times;
or a raw `cron` string for power users (mutually exclusive with recurrence).
Action on fire: heartbeat (wake only, no card) | routine (recurring work) | reminder.

Firing uses the host clock + timezone; schedules persist and re-arm on restart.
Board de-duplication for repeated routine fires is deferred (tuned later).
