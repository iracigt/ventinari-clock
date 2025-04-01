import pygame
import numpy as np
import time
from datetime import datetime


# State machine
state = 0

# transitions = np.array([
#     [ 1, 14, 0, 1 ],
#     [ 2, 1, 13, 0 ],
#     [ 0, 2, 0, 14 ],
#     [ 14, 1, 1, 0 ],
# ])/16

# basically speed=1.0
transitions = np.array([
    [ 1, 14, 0, 1 ],
    [ 1, 1, 13, 1 ],
    [ 0, 0, 2, 14 ],
    [ 14, 1, 1, 0 ],
])/16


acc = np.zeros(4)

# Compute steady state transition matrix
steady_state = np.linalg.matrix_power(transitions, 1000)
print(steady_state)
print("speed =", np.sum(steady_state[:,0]))
print("avg drift / day =", (np.sum(steady_state[:,0]) - 1)* 3600*24)

for _ in range(20):
    for i in range(60*4):
            state = np.where(np.cumsum(transitions[state]) >= np.random.rand())[0][0]
            acc[state] += 1
    print('simulated minute', acc[0] - 60, acc[0] / np.sum(acc) * 4, acc / np.sum(acc))
    acc = np.zeros(4)

for i in range(3600*24*4):
        state = np.where(np.cumsum(transitions[state]) >= np.random.rand())[0][0]
        acc[state] += 1
print('simulated day', acc[0] - 3600*24, acc[0] / np.sum(acc) * 4, acc / np.sum(acc))
acc = np.zeros(4)

# Initialize pygame
pygame.init()

# Screen dimensions
WIDTH, HEIGHT = 800, 600
screen = pygame.display.set_mode((WIDTH, HEIGHT))
pygame.display.set_caption("Clock")

# Fonts and colors
font = pygame.font.Font(None, 256)
bg_color = (0, 0, 0)
text_color = (255, 255, 255)

# Generate click sound
sound_array = np.int16(np.sin(np.linspace(0, 4*np.pi, 100)) * 2**14)
stereo_array = np.column_stack((sound_array, sound_array))  # Make it 2D for stereo
click_sound = pygame.mixer.Sound(buffer=pygame.sndarray.make_sound(stereo_array))

# Clock for controlling frame rate
clock = pygame.time.Clock()

running = True
last_second = -1

while running:
    for event in pygame.event.get():
        if event.type == pygame.QUIT:
            running = False

    # Get current time
    now = datetime.now()
    current_time = now.strftime("%H:%M:%S")


    # Update the state machine
    state = np.where(np.cumsum(transitions[state]) >= np.random.rand())[0][0]
    acc[state] += 1
    print(acc[0] / np.sum(acc) * 4, acc / np.sum(acc))

    if state == 0:
        click_sound.play()
        text_color = (255, 255, 255)
    elif state == 1:
        text_color = (255, 0, 0)
    elif state == 2:
        text_color = (0, 255, 0)
    elif state == 3:    
        text_color = (0, 0, 255)    

    # Check if the second has changed
    if now.second != last_second:
        last_second = now.second

    # Draw the current time
    screen.fill(bg_color)
    text_surface = font.render(str(state), True, text_color)
    text_rect = text_surface.get_rect(center=(WIDTH // 2, HEIGHT // 2))
    screen.blit(text_surface, text_rect)

    # Update the display
    pygame.display.flip()

    # Cap the frame rate
    clock.tick(4)

    

pygame.quit()