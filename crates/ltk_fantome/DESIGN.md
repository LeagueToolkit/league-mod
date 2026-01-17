## Fantome Format Structure

The library creates ZIP files with this structure, following the [official Fantome specification](https://github.com/LeagueToolkit/Fantome/wiki/Mod-File-Format):

```
my_mod_1.0.0.fantome
├── META/
│   ├── info.json            # Mod metadata
│   ├── README.md            # Project documentation (optional)
│   └── image.png            # Mod thumbnail (optional)
│                                                                    
├── WAD/                     # Base Layer (required)                                                            
│    ├── Aatrox.wad.client/                                                                                     
│    │   ├── data/                                                                                     
│    │   └── assets/
│ 	 └── Map11.wad.client/
│	     ├── data/
│	     └── assets/       
│																									 
└── WAD_pink/                # Pink Chroma Layer                                                                                                    
	 └── Aatrox.wad.client/                                                                                                 
		 ├── data/                                                                                                          
		 └── assets/
```

### Metadata Structure

The `info.json` file contains metadata in the format expected by Fantome:

```json
{
  "Name": "Display Name",
  "Author": "Author Name",
  "Version": "1.0.0",
  "Description": "Mod description",
  "Layers": {                        
	"base": {                     # "WAD" folder (required)
	  "Name": "base",
	  "Priority": 0,
      "StringOverrides": {    
 	    "field1": "New String"
 	    "field2": "New String"
      } 
	}
	"Pink": {
	  "Name": "Pink Chroma",
	  "Priority": 10,
      "StringOverrides": {    
 	    "field3": "New String"
      } 
	}
  }
  "Groups": {                        
	"base": {
	  "Kind": "Inclusive",
	  "Members": []
	}
	"Group": {
	  "Kind": "Inclusive",
	  "Members": ["Layer"]
	}
  }
}
```

## StringOverrides

String overrides define new string values for specific fields.
Full list of fields and their default value are located in file `data/menu/en_us/lol.stringtable`, that is located in `Localized/Global.{locale}.wad.client`
Approach of shipping only overrides in mod is due to need to have `lol.stringtable` file *ALWAYS* up to date.
Tool that allows converting between `.stringtable` and `.json` is [Rion](https://github.com/Roshaless/Rion/releases)

## Limitations

- **Fixed structure**: Must follow the exact WAD folder structure expected by League of Legends
