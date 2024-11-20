# awesome-generator

Tool to integrate with GitHub, GitLab and Connect IQ. It is currently helping
automate the process of updating the README but finding, adding or removing
resources is still a manual process.

## Generate README

The main purpose of this tool is to generate the [`README.md`] by reading
[`awesome.toml`] and fetching description, last activity and more from GitHub or
GitLab.

## Search

The tool supports searching the [Connect IQ app library] and print any
application that has a website URL linked which usually points to the source
code.

```sh
› cargo run search sailing

 Change date             | Type      | URL
-------------------------+-----------+-------------------------------------------------------------
 2024-06-08 22:04:10 UTC | DeviceApp | https://github.com/pintail105/SailingTools
 2018-12-05 15:09:37 UTC | DeviceApp | https://github.com/antgoldbloom/VMG-Connect-IQ-App
 2023-10-25 07:04:22 UTC | DeviceApp | https://github.com/AlexanderLJX/Sailing-Windsurfing-Foiling
 2024-09-02 12:43:09 UTC | DeviceApp | https://github.com/dmrrlc/connectiq-sailing
 2018-12-05 15:28:10 UTC | DeviceApp | https://github.com/spikyjt/SailingTimer
 2024-11-04 08:04:27 UTC | DeviceApp | https://github.com/Laverlin/Yet-Another-Sailing-App
 2021-04-16 09:25:18 UTC | DeviceApp | https://github.com/alexphredorg/ConnectIqSailingApp
 2023-10-25 07:37:00 UTC | DeviceApp | https://github.com/pukao/GarminSailing
 2024-02-14 00:30:24 UTC | DataField | https://github.com/Fra-Sti/Sailing-Instrument
 2023-12-04 06:43:39 UTC | DeviceApp | https://seatouch.dev/#/foilstart
 2024-07-11 05:05:21 UTC | DeviceApp | https://github.com/zlelik/ConnectIqSailingApp
```

## Compare

To make it easy to maintain and discover new resources the tool can compare a
search result with the `awesome.toml` and print any diff in their respective
category.

```sh
› cargo run compare gitlab

Found 8 URLs not in list

[watch_faces]
"https://gitlab.com/knusprjg/wherearemyglasses" = {}
"https://gitlab.com/HankG/GarminConnectIQ" = {}
"https://gitlab.com/aleixq/connect-iq-analog-red" = {}

[data_fields]
"https://gitlab.com/nz_brian/garmin.pacer" = {}
"https://gitlab.com/nz_brian/garmin.datafield.timeanddistance" = {}
"https://gitlab.com/twk3/currento2" = {}

[device_apps]
"https://gitlab.com/ApnoeMax/apnoe-statik-timer" = {}
"https://gitlab.com/btpv/zermeloforgarmin/" = {}
```

[`README.md`]: ../README.md
[`awesome.toml`]: ./awesome.toml
[Connect IQ app library]: https://apps.garmin.com
