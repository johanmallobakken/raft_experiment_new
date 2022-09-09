import re
import matplotlib.pyplot as plt
import collections
from matplotlib.pyplot import figure


figure(figsize=(3, 2), dpi=100)

file1 = open('state.txt', 'r')
Lines = file1.readlines()

logs = collections.defaultdict(list)
simulation_steps = list()
  
# Strips the newline character
for line in Lines:
    if line.startswith("RaftState"):
        id = re.search(r'id:\s(.*)', line)
        log_committed = re.search(r'log_last_index:\s(.*)', line)

        id_int = int(id.groups()[0].split()[0].strip(","))
        log_int = int(log_committed.groups()[0].split()[0].strip(","))

        logs[id_int].append(log_int)
    elif line.startswith("BreakLink"):
        broken_link_step = int(line.split()[1])
    else:
        count = int(line.split()[0])
        print(count)
        simulation_steps.append(count)



fig, ax = plt.subplots()

if broken_link_step: 
    for break_index in range(len(simulation_steps)):
        if simulation_steps[break_index] >= broken_link_step:
            break

    max_y_break_point = 0
    for i in range(len(logs)):
        max_y_break_point = max(logs[i+1][break_index], max_y_break_point) 
        ax.plot(simulation_steps, logs[i+1], label = ("Node " + str(i+1)))

    ax.annotate(
        "Partition Point", 
        xy=(broken_link_step, ax.get_ylim()[0]), 
        xytext=(broken_link_step, ax.get_ylim()[1] + ax.get_ylim()[1]*0.1), 
        arrowprops = dict(facecolor='black', width=0.1, headwidth=0.1))
else:
    for i in range(len(logs)):
        ax.plot(simulation_steps, logs[i+1], label = ("Node " + str(i+1)))


plt.xlabel('Simulation steps', fontsize=20)
# naming the y axis
plt.ylabel('Log lengths', fontsize=20)
# giving a title to my graph
plt.title('Log lengths over simulation step time', fontsize=20)

plt.xticks(fontsize=20)
plt.yticks(fontsize=20)
plt.legend(fontsize=20)

# function to show the plot
plt.show()