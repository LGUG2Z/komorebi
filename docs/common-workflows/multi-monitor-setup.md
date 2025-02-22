# Multiple Monitors Setup

You can setup komorebi to work with multiple monitors. To do so, first you start by setting up multiple monitor 
configurations on your `komorebi.json` config file. If you've used the [`komorebic quickstart`](../cli/quickstart.md) 
command you'll already have a `komorebi.json` config file with one monitor config setup. Open this file and look 
for the `"monitors":` line, you should find something like this:
```json
  "monitors": [
    {
      "workspaces": [
        {
          "name": "I",
          "layout": "BSP"
        },
        {
          "name": "II",
          "layout": "VerticalStack"
        },
        {
          "name": "III",
          "layout": "HorizontalStack"
        },
        {
          "name": "IV",
          "layout": "UltrawideVerticalStack"
        },
        {
          "name": "V",
          "layout": "Rows"
        },
        {
          "name": "VI",
          "layout": "Grid"
        },
        {
          "name": "VII",
          "layout": "RightMainVerticalStack"
        }
      ]
    }
  ]
```

For this example we will remove some workspaces to simplify the config so it is easier to look at, but feel free to 
setup as many workspaces per monitor as you'd like! So here is the same configuration with only 3 workspaces.
```json
  "monitors": [
    {
      "workspaces": [
        {
          "name": "I",
          "layout": "BSP"
        },
        {
          "name": "II",
          "layout": "VerticalStack"
        },
        {
          "name": "III",
          "layout": "HorizontalStack"
        },
      ]
    }
  ]
```
Now to add another monitor config you would simply do this:
```json
  "monitors": [
    {
      "workspaces": [
        {
          "name": "I",
          "layout": "BSP"
        },
        {
          "name": "II",
          "layout": "VerticalStack"
        },
        {
          "name": "III",
          "layout": "HorizontalStack"
        },
      ]
    },
    {
      "workspaces": [
        {
          "name": "1",
          "layout": "BSP"
        },
        {
          "name": "2",
          "layout": "VerticalStack"
        },
        {
          "name": "3",
          "layout": "HorizontalStack"
        },
      ]
    }
  ]
```
This means we now have 2 monitor configurations. We have the first monitor configuration, which is index 0 (*usually 
on programming languages the first item of a list starts with index 0*), this configuration has 3 workspaces with names 
"I", "II" and "III". Then the 2nd monitor configuration, which is index 1, also has 3 workspaces with names "1", "2", 
and "3" (you should always give different names to your workspaces!).

And it is as simple as that, now if you start komorebi with two monitors connected the main monitor will use the configuration 
with index 0 and the secondary monitor will use the configuration with index 1.

Now lets say you have more monitors or you want to make sure that the correct configuration is always applied to the right 
monitor, then you will want to use the `display_index_preferences`. To do that first open up a terminal and type the following 
command: [`komorebic monitor-info`](../cli/monitor-information.md). This command will give you the information about your connected 
monitors, you want to look up the `serial_number_id`. You should get something like this:
```
❯ komorebic monitor-info
[
  {
    "id": 6620935,
    "name": "DISPLAY1",
    "device": "BOE0A1C",
    "device_id": "BOE0A1C-5&a2bea0b&0&UID512",
    "serial_number_id": "0",
    "size": {
      "left": 0,
      "top": 0,
      "right": 1920,
      "bottom": 1080
    }
  },
  {
    "id": 181932057,
    "name": "DISPLAY2",
    "device": "VSC8C31",
    "device_id": "VSC8C31-5&18560b1f&0&UID4356",
    "serial_number_id": "UEP174021562",
    "size": {
      "left": 0,
      "top": -1080,
      "right": 1920,
      "bottom": 1080
    }
  }
]
```
In this case the setup is a laptop with a secondary monitor connected. You'll need to figure out which monitor is which, 
usually the display name's number should be similar to the numers you can find on `Windows Settings -> System -> Display`.

If you have trouble with this step you can always jump on Discord and ask for help (create a `Support` thread).

Once you know which monitor is which, you want to look up their `serial_number_id` to use that on `display_index_preferences`, 
you can also use the `device_id`, it accepts both however there have been reported cases where the `device_id` changes after 
a restart while the `serial_number_id` doesn't.

So with the example above, we want the laptop to always use the configuration index 0 and the other monitor to use configuration 
index 1, so we map the configuration index number to the monitor `serial_number_id`/`device_id` like this:
```json
"display_index_preferences": [
    "0": "0",
    "1": "UEP174021562"
]
```
Again you could also have used the `device_id` like this:
```json
"display_index_preferences": [
    "0": "BOE0A1C-5&a2bea0b&0&UID512",
    "1": "VSC8C31-5&18560b1f&0&UID4356"
]
```

You should add this `display_index_preferences` somewhere on your `komorebi.json` file. If you find that something is not working as 
expected you can try to use the command `komorebic check`, you can also use VSCode for example to see if there is any mistake with 
your `komorebi.json` file or, as always, you can pop on Discord and ask for help there.

> [!IMPORTANT]
> 
> **When using multiple monitors it is recommended to always set the `display_index_preferences`. If you don't you might get some 
undefined behaviour.**

If you would like to run multiple instances of `komorebi-bar` to target different monitors, it is possible to do so
using the `bar_configurations` array in your `komorebi.json` configuration file. You can refer to the 
[multiple-bar-instances](multiple-bar-instances.md) documentation.

In this case it is specially important to use the `display_index_preferences`, because if you don't and you have 3 or more monitors and 
disconnect some monitor then the bars for the monitors get shifted around.

For instance imagine the setup with 3 monitors (A, B and C), the user sets this config:
```jsonc
// HOME_MONITOR_1_BAR.json
"monitor_index": 0,
//...
```
```jsonc
// HOME_MONITOR_2_BAR.json
"monitor_index": 1,
//...
```
```jsonc
// WORK_MONITOR_1_BAR.json
"monitor_index": 2,
//...
```
```jsonc
"display_index_preferences": [
  "0": "MONITOR_1_ID",
  "1": "MONITOR_2_ID",
  "2": "MONITOR_3_ID",
],
"bar_configurations": [
  "path/to/bar_config_1.json", // this bar uses "monitor_index": 0,
  "path/to/bar_config_2.json", // this bar uses "monitor_index": 1,
  "path/to/bar_config_3.json", // this bar uses "monitor_index": 2,
]
```

This looks all good and normal! Komorebi uses an internal map to keep track of monitor to config indices, this map is called 
`monitor_usr_idx_map` it is an internal variable to komorebi that you don't need to do anything with but you can see it with 
the [`komorebic state`](../cli/state.md) command (in case you need to debug something).
At first, komorebi will load all monitors and set the internal index map (`monitor_usr_idx_map`) as:
```jsonc
[
  "0": 0, // This is monitor A
  "1": 1, // This is monitor B
  "2": 2, // This is monitor C
]
```
Which kind of seems unnecessary, but imagine that then you disconnect monitor B (or it goes to sleep). Then komorebi will only 
have 2 monitors with index 0 and 1, so the above map will be updated to this:
```jsonc
[
  "0": 0, // This is monitor A
  "2": 1, // This is now monitor C, because monitor B disconnected
]
```
So now the bar supposed to be for monitor B which was looking for index "1" on that map doesn't see it and knows it should be 
disabled. And the bar for monitor C looks at that map and knows that it's index "2" now maps to index 1 so it uses that index 
internally to get all the correct values about the monitor!

If you didn't have the `display_index_preferences` setup then when you disconnected the monitor B komorebi wouldn't know how 
to map the indices and would use default behaviour which would result in a map like this:
```jsonc
[
  "0": 0, // This is monitor A
  "1": 1, // This is monitor C, because monitor B disconnected. However the bars will think it is monitor B because it has index "1"
]
```

If you setup the `display_index_preferences` then both komorebi and the bar will be aware of that and the bars will still show 
on the correct monitors.


# Multiple Monitors on different machines

You can use the same `komorebi.json` to configure two different setups and then synchronize your config across machines. But if you do this 
it is important to be aware of a few things. Firt of all using `display_index_preferences` is a must in this case. You will need to get the 
`serial_number_id` or `device_id` of all the monitors of all your setups. With that information you would then setup your config like this:

```jsonc
"display_index_preferences": [
  "0": "HOME_MONITOR_1_ID",
  "1": "HOME_MONITOR_2_ID",
  "2": "WORK_MONITOR_1_ID",
  "3": "WORK_MONITOR_2_ID",
],
"monitors": [
  { // HOME_MONITOR_1
    "workspaces": [
      // ...
    ]
  },
  { // HOME_MONITOR_2
    "workspaces": [
      // ...
    ]
  },
  { // WORK_MONITOR_1
    "workspaces": [
      // ...
    ]
  }
  { // WORK_MONITOR_2
    "workspaces": [
      // ...
    ]
  }
]
```

> [!NOTE]
> 
> *You can't use the same config on two different monitors, you have to make a duplicated config for each monitor!*

Then on the bar configs you need to set the bar's monitor index like this:
```jsonc
// HOME_MONITOR_1_BAR.json
"monitor_index": 0,
//...
```
```jsonc
// HOME_MONITOR_2_BAR.json
"monitor_index": 1,
//...
```
```jsonc
// WORK_MONITOR_1_BAR.json
"monitor_index": 2,
//...
```
```jsonc
// WORK_MONITOR_2_BAR.json
"monitor_index": 3,
//...
```

Even tough you will only ever have 2 monitors connected at a time and that they'll have index 0 and 1, the above config will still work on 
both places since komorebi will apply the correct config to the loaded monitors and will create a map of "user" index (the index defined on 
the config) to actual monitor index and the bar will use that map to know if it should be enabled or not and where to be drawn!

For instance at work that `monitor_usr_idx_map` will be:
```
[
  "2": 0,
  "3": 1,
]
```
This `monitor_usr_idx_map` is internal to komorebi and automatically updated for you so you don't need to care about it, it is just so you 
understand what is going on under the hood.


### Things you need to know about this setup and multiple monitors:

* If you are using a laptop connected to one monitor at work and a different one at home you need to understand that the work monitor and 
the home monitor are different monitors for komorebi. So when you disconnect from work, komorebi will keep the work monitor cached. You can 
still use your laptop alone without any monitor and if you need a window that was on the other monitor you can press the taskbar icon or use 
`alt + tab` to bring it to focus and that window will now be part of the laptop monitor. If you then reconnect the work monitor the cached 
version will be applied with all its windows (except any window you might have moved to another monitor like it was just described). If, however,
instead of reconnecting the work monitor you connect the home monitor, then the work monitor will still remain cached and komorebi will load the 
home monitor cache if it exists.

* Sometimes when you disconnect/reconnect a monitor the event might be missed by komorebi, meaning that Windows will show you both monitors 
but komorebi won't know about the existence of one of them. If you notice some weird behaviour, always run the [`komorebic monitor-info`](../cli/monitor-information.md) 
command. If this is the case, then that command won't show you one of the monitors. To fix this you can try disconnecting and reconnecting 
the monitor again. After the latest changes to komorebi this hasn't happened yet, but considering the finicky behaviour of Windows display 
events this can potentially happen so it is important to know how to deal with it.
