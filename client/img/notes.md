# Portrait Sprite

The portraits come in as a sprite. Meaning, all the portraits for all delegates are in one single file.
To create the sprite sheet be mindeful of these pionts:
- Each portrait is a 2:3 aspect ratio
- The width of each portrait MUST be 600px
- The CSS will slice out the sprites

# Generation Script
There is a helper script to generate the sprite sheet so that it's easier to manage images.
Images filenames which match `leader*` are placed into the sprite sheet.
The original image is left untouched.

Requirement:
- Python3 
- Conda 

Steps:
- Use `conda env create -f environment.yml` if you do not have the environment yet.
- Activate the conda env `conda activate model-un`
- Run `python3 generate_sprites.py`



Indian Princess - Standing infront of a mine of rubies 
Jamacan -  leader of agriculture standing infront of a vast forest of sugar cane
Ivrory coast African - Building a mighty merchant marine 
Native american - Leader of a peaceful group infront of 
Middle Eastern Saudi - Standing infront of golden shops and bazzar