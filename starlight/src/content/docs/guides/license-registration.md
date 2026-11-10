---
title: License Registration
description: A license registration guide for komorebi.
---

komorebi [identifies corporate devices](https://lgug2z.com/articles/normalize-identifying-corporate-devices-in-your-software/) [on a best-effort
basis](https://lobste.rs/s/vd7dgj/normalize_identifying_corporate) through the
detection of mobile device management enrollment.

## License Registration

If you are using komorebi on a device enrolled in mobile device management, you
will receive a pop-up when you run `komorebic start` reminding you that the
[Komorebi License](https://github.com/LGUG2Z/komorebi-license) does not permit
any kind of commercial use.

You can remove this pop-up by running `komorebic license <email>` with the email
associated with your Individual Commercial Use License. A single HTTP request
will be sent with the given email address to verify license validity.

## Students on Windows Machines

If you have an device provided by your educational institution, or a personal
device which you have been forced to enroll in "Bring Your Own Device" (BYOD)
MDM, you can email me (my_github_username _at_ fastmail _dot_ com) from the
address associated with your institution with the subject "komorebi - student
with an MDM device", and I will be able to remove the splash intended for
corporate users

This is currently a manual process - most days this shouldn't take more than
12h, and you will receive an email reply from me when the process is complete.

If you haven't had a reply to your email within 24h you can reach out to me on
[Discord](https://discord.gg/mGkn66PHkx).

## Edge Cases on Windows

There are always edge cases, and if you are using komorebi for personal use and
think you have been caught up in one of them you can email me with a screenshot
of the output from `dsregcmd /status` and a few lines about why the device you
are using is enrolled in (BYOD) MDM and I can usually add an override for you.
